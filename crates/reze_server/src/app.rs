use std::cmp::Ordering;
use std::fmt::Write;
use std::time::Instant;

use bytes::Bytes;
use futures_util::future::{BoxFuture, join_all};
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use percent_encoding::percent_decode_str;
use sha1::{Digest, Sha1};
use tracing::{Instrument, field, info_span};

use crate::html::escape_text;
use crate::render::{Page, render};
use crate::{Chunk, Loader, Manifest, Params, Route, RouteError, Segment, Table, Value};

type Erased<C> = Box<
    dyn for<'a> Fn(&Params, &'a C) -> BoxFuture<'a, crate::Result<Box<dyn Value>>> + Send + Sync,
>;

/// The application: the generated route table, the Rust loaders, and the client build.
pub struct App<C> {
    table: &'static Table,
    /// Routes in match order.
    ordered: Vec<&'static Route>,
    loaders: Vec<Option<Erased<C>>>,
    manifest: Option<Manifest>,
    server_timing: bool,
}

fn compare(a: &Route, b: &Route) -> Ordering {
    for index in 0..a.segments.len().max(b.segments.len()) {
        match (a.segments.get(index), b.segments.get(index)) {
            (Some(left), Some(right)) => match left.rank().cmp(&right.rank()) {
                Ordering::Equal => {}
                order => return order,
            },
            (left, _) => return if left.is_some() { Ordering::Greater } else { Ordering::Less },
        }
    }
    Ordering::Equal
}

fn decode(part: &str) -> String {
    percent_decode_str(part).decode_utf8_lossy().into_owned()
}

/// Matches `parts` against `route`, and returns the params and the canonical path.
fn matches(route: &Route, parts: &[&str]) -> Option<(Params, String)> {
    let mut params = Params::new();
    let mut canonical: Vec<&str> = Vec::new();
    let mut index = 0;
    for segment in route.segments {
        let part = parts.get(index).copied();
        match *segment {
            Segment::Static(name) => {
                if part?.to_lowercase() != name {
                    return None;
                }
                canonical.push(name);
                index += 1;
            }
            Segment::Param(name) => {
                let part = part?;
                params.insert(name, decode(part));
                canonical.push(part);
                index += 1;
            }
            Segment::Optional(name) => {
                if let Some(part) = part {
                    params.insert(name, decode(part));
                    canonical.push(part);
                    index += 1;
                }
            }
            Segment::Splat(name) => {
                if index >= parts.len() {
                    return None;
                }
                let rest: Vec<String> = parts[index..].iter().map(|part| decode(part)).collect();
                params.insert(name, rest.join("/"));
                canonical.extend(&parts[index..]);
                index = parts.len();
            }
        }
    }
    (index == parts.len()).then(|| (params, format!("/{}", canonical.join("/"))))
}

fn built_in(error: &RouteError) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{0}</title></head><body><h1>{0}</h1><p>{1}</p></body></html>"#,
        error.status,
        escape_text(&error.message)
    )
}

fn response(status: u16, headers: HeaderMap, body: impl Into<Bytes>) -> Response<Bytes> {
    let mut response = Response::new(body.into());
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    *response.headers_mut() = headers;
    response
}

fn plain(status: u16, body: &'static str) -> Response<Bytes> {
    response(status, HeaderMap::new(), body)
}

/// Whether a post comes from another site, by `Sec-Fetch-Site`, then `Origin`.
fn cross_site(headers: &HeaderMap) -> bool {
    let get = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    if let Some(site) = get("sec-fetch-site") {
        return site == "cross-site";
    }
    match get("origin") {
        Some(origin) => {
            let host = origin.split_once("://").map_or(origin, |(_, rest)| rest);
            Some(host) != get("host")
        }
        None => false,
    }
}

/// The ETag of a document: base64url of the first 12 bytes of its SHA-1.
fn etag(document: &str) -> String {
    format!("\"{}\"", base64_url::encode(&Sha1::digest(document.as_bytes())[..12]))
}

mod base64_url {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    /// Base64url of `bytes`, whose length is a multiple of 3.
    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
            for shift in [18, 12, 6, 0] {
                out.push(ALPHABET[(n >> shift & 63) as usize] as char);
            }
        }
        out
    }
}

/// What the loaders of one request write: their data, the shallowest error, and their timings.
struct Run<'r> {
    data: &'r mut [Option<Box<dyn Value>>],
    error: &'r mut Option<(usize, RouteError)>,
    timings: &'r mut String,
}

/// Appends a `Server-Timing` metric of `seconds`.
fn timing(timings: &mut String, name: &str, seconds: f64) {
    if !timings.is_empty() {
        timings.push_str(", ");
    }
    let _ = write!(timings, "{name};dur={:.1}", seconds * 1000.0);
}

impl<C: Send + Sync + 'static> App<C> {
    pub fn new(table: &'static Table) -> Self {
        let mut ordered: Vec<&'static Route> = table.routes.iter().collect();
        ordered.sort_by(|a, b| compare(a, b));
        Self {
            table,
            ordered,
            loaders: table.nodes.iter().map(|_| None).collect(),
            manifest: None,
            server_timing: cfg!(feature = "dev"),
        }
    }

    /// Sets the loader of route node `index`, which the generated setters call.
    pub fn set_loader<P, T, F>(&mut self, index: usize, params: fn(&Params) -> P, loader: F)
    where
        P: Send + 'static,
        T: Value,
        F: for<'a> Loader<'a, C, P, T> + 'static,
    {
        self.loaders[index] = Some(Box::new(move |raw, context| {
            let future = loader.load(params(raw), context);
            Box::pin(async move { future.await.map(|value| Box::new(value) as Box<dyn Value>) })
        }));
    }

    pub fn set_manifest(&mut self, manifest: Manifest) {
        self.manifest = Some(manifest);
    }

    /// Writes loader and render durations into `Server-Timing`: on with the `dev` feature, and
    /// opt-in otherwise, because it reveals timing to every client.
    pub fn set_server_timing(&mut self, on: bool) {
        self.server_timing = on;
    }

    /// Answers one request. The body is unread: the Rust server has no mutations.
    pub async fn handle<B>(&self, request: Request<B>, context: &C) -> Response<Bytes> {
        let (parts, _) = request.into_parts();
        let span = info_span!(
            "request",
            method = %parts.method,
            path = parts.uri.path(),
            status = field::Empty
        );
        let response = self.respond(&parts, context).instrument(span.clone()).await;
        span.record("status", response.status().as_u16());
        response
    }

    async fn respond(&self, request: &http::request::Parts, context: &C) -> Response<Bytes> {
        let path = request.uri.path();
        let query: Vec<(String, String)> =
            form_urlencoded::parse(request.uri.query().unwrap_or("").as_bytes())
                .into_owned()
                .collect();
        #[cfg(feature = "dev")]
        if path == "/__reze/load" {
            return self.load_json(&query, context).await;
        }
        let get = |name: &str| query.iter().find(|(key, _)| key == name).map(|(_, v)| v.as_str());
        let build = get("_b");
        let segment = match get("_s").map(str::trim) {
            None | Some("") => 0.0,
            Some(text) => text.parse::<f64>().unwrap_or(f64::NAN),
        };
        if let Some(build) = build
            && Some(build) != self.manifest.as_ref().map(|m| m.build.as_str())
        {
            return plain(409, "Conflict");
        }
        let search: String = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(query.iter().filter(|(key, _)| key != "_b" && key != "_s"))
            .finish();
        let search = if search.is_empty() { search } else { format!("?{search}") };
        let method = &request.method;

        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let span = info_span!("match", pattern = field::Empty);
        let found = span.in_scope(|| {
            self.ordered.iter().find_map(|route| {
                let (params, canonical) = matches(route, &parts)?;
                Some((*route, params, canonical))
            })
        });
        if let Some((route, _, canonical)) = &found {
            span.record("pattern", route.pattern);
            if canonical != path && method == Method::GET {
                let mut headers = HeaderMap::new();
                if let Ok(location) = HeaderValue::from_str(&format!("{canonical}{search}")) {
                    headers.insert(header::LOCATION, location);
                }
                return response(308, headers, Bytes::new());
            }
        }
        let head = method == Method::HEAD;
        let mut timings = String::new();
        let Some((route, params, _)) = found else {
            let page = Page {
                table: self.table,
                chain: self.table.root,
                path,
                search: &search,
                query: &query,
                params: &Params::new(),
                data: &[],
                error: Some((self.table.root.len(), RouteError::not_found())),
            };
            let rendered = self.render(&page, None, 0, &mut timings);
            return self.finish(rendered, HeaderMap::new(), &timings, head, request);
        };
        let chain = route.chain;
        let from =
            if method == Method::GET && segment.fract() == 0.0 && segment < chain.len() as f64 {
                segment.max(0.0) as usize
            } else {
                0
            };
        if method != Method::GET && method != Method::HEAD && method != Method::POST {
            let mut headers = HeaderMap::new();
            headers.insert(header::ALLOW, HeaderValue::from_static("GET, HEAD, POST"));
            return response(405, headers, "Method Not Allowed");
        }
        if method == Method::POST {
            return if cross_site(&request.headers) {
                plain(403, "Forbidden")
            } else {
                plain(404, "Not Found")
            };
        }

        let mut data: Vec<Option<Box<dyn Value>>> = chain.iter().map(|_| None).collect();
        let mut error = None;
        let mut state = Run { data: &mut data, error: &mut error, timings: &mut timings };
        self.run(route, (from..chain.len()).collect(), &params, context, &mut state).await;
        let mut headers = HeaderMap::new();
        if let Some(seconds) =
            chain.iter().filter_map(|&node| self.table.nodes[node].revalidate).min()
            && let Ok(value) =
                HeaderValue::from_str(&format!("s-maxage={seconds}, stale-while-revalidate"))
        {
            headers.insert(header::CACHE_CONTROL, value);
        }
        if from > 0 {
            let page = Page {
                table: self.table,
                chain,
                path,
                search: &search,
                query: &query,
                params: &params,
                data: &data,
                error: error.clone(),
            };
            let rendered = self.render(&page, route.chunk, from, &mut timings);
            if rendered.is_ok() {
                headers.insert("rz-segment", HeaderValue::from(from));
                return self.finish(rendered, headers, &timings, head, request);
            }
            let mut state = Run { data: &mut data, error: &mut error, timings: &mut timings };
            self.run(route, (0..from).collect(), &params, context, &mut state).await;
        }
        let page = Page {
            table: self.table,
            chain,
            path,
            search: &search,
            query: &query,
            params: &params,
            data: &data,
            error,
        };
        let rendered = self.render(&page, route.chunk, 0, &mut timings);
        self.finish(rendered, headers, &timings, head, request)
    }

    /// Runs the loaders of `depths` together, and keeps the error of the shallowest that fails.
    async fn run(
        &self,
        route: &Route,
        depths: Vec<usize>,
        params: &Params,
        context: &C,
        state: &mut Run<'_>,
    ) {
        let loads = depths.iter().map(|&depth| {
            let node = route.chain[depth];
            let loader = self.loaders[node].as_ref();
            let wanted = self.table.nodes[node].loader;
            let span = info_span!("loader", route = route.pattern, depth);
            async move {
                let start = Instant::now();
                let result = match (wanted, loader) {
                    (false, _) => None,
                    (true, Some(loader)) => Some(loader(params, context).await),
                    (true, None) => {
                        tracing::error!(
                            file = self.table.nodes[node].file,
                            "no Rust loader is set"
                        );
                        Some(Err(RouteError::internal()))
                    }
                };
                (result, start.elapsed())
            }
            .instrument(span)
        });
        for (depth, (result, elapsed)) in depths.iter().zip(join_all(loads).await) {
            if result.is_some() && self.server_timing {
                timing(state.timings, &format!("loader-{depth}"), elapsed.as_secs_f64());
            }
            match result {
                None => {}
                Some(Ok(value)) => state.data[*depth] = Some(value),
                Some(Err(failure)) if state.error.as_ref().is_none_or(|(at, _)| at > depth) => {
                    *state.error = Some((*depth, failure));
                }
                Some(Err(_)) => {}
            }
        }
    }

    fn render(
        &self,
        page: &Page<'_>,
        chunk: Option<Chunk>,
        from: usize,
        timings: &mut String,
    ) -> crate::Result<(String, u16)> {
        let span = info_span!("render", bytes = field::Empty);
        let start = Instant::now();
        let rendered = span.in_scope(|| render(page, chunk, self.manifest.as_ref(), from));
        if self.server_timing {
            timing(timings, "render", start.elapsed().as_secs_f64());
        }
        if let Ok((document, _)) = &rendered {
            span.record("bytes", document.len());
        }
        rendered
    }

    fn finish(
        &self,
        rendered: crate::Result<(String, u16)>,
        mut headers: HeaderMap,
        timings: &str,
        head: bool,
        request: &http::request::Parts,
    ) -> Response<Bytes> {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
        if !timings.is_empty()
            && let Ok(value) = HeaderValue::from_str(timings)
        {
            headers.insert("server-timing", value);
        }
        let (document, status) = match rendered {
            Ok(rendered) => rendered,
            Err(error) => (built_in(&error), error.status),
        };
        if status == 200 {
            let tag = etag(&document);
            let matched = request
                .headers
                .get(header::IF_NONE_MATCH)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == tag));
            if let Ok(value) = HeaderValue::from_str(&tag) {
                headers.insert(header::ETAG, value);
            }
            if matched {
                return response(304, headers, Bytes::new());
            }
        }
        response(status, headers, if head { Bytes::new() } else { Bytes::from(document) })
    }

    /// The `dev` endpoint `GET /__reze/load?m=<file>&<params>`: the data of one loader as JSON,
    /// for the JS dev server.
    #[cfg(feature = "dev")]
    async fn load_json(&self, query: &[(String, String)], context: &C) -> Response<Bytes> {
        let mut file = None;
        let mut params = Params::new();
        for (name, value) in query {
            if name == "m" {
                file = Some(value.as_str());
            } else {
                params.insert(name.clone(), value.clone());
            }
        }
        let node = self.table.nodes.iter().position(|node| Some(node.file) == file);
        let Some(loader) = node.and_then(|node| self.loaders[node].as_ref()) else {
            return plain(404, "Not Found");
        };
        match loader(&params, context).await {
            Ok(value) => {
                let mut headers = HeaderMap::new();
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
                response(200, headers, value.json())
            }
            Err(error) => response(error.status, HeaderMap::new(), error.message),
        }
    }
}
