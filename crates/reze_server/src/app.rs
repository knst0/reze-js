use std::cmp::Ordering;
use std::fmt::Write;
use std::future::poll_fn;
use std::sync::Arc;
use std::task::Poll;
use std::time::Instant;

use bytes::Bytes;
use futures_util::future::{BoxFuture, join_all};
use futures_util::stream::{self, StreamExt};
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use percent_encoding::percent_decode_str;
use sha1::{Digest, Sha1};
use tracing::{Instrument, field, info_span};

use crate::html::escape_text;
use crate::render::{Page, Part, Rendered, Submission, render, render_part};
use crate::{
    Body, Chunk, Guard, Loader, Manifest, Mutation, Params, Route, RouteError, Segment, Table,
    Value,
};

type Erased<C> = Box<
    dyn for<'a> Fn(&Params, &'a C) -> BoxFuture<'a, crate::Result<Box<dyn Value>>> + Send + Sync,
>;

type ErasedGuard<C> = Box<
    dyn for<'a> Fn(&Params, &'a http::request::Parts, &'a C) -> BoxFuture<'a, crate::Result<()>>
        + Send
        + Sync,
>;

/// The body of a mutation post: JSON from `.call`, or the fields of a form.
enum Posted {
    Json(reze::Json),
    Form(Vec<(String, String)>),
}

/// A mutation's input checked against its type: the mutation to run, or why it cannot.
enum Decoded<'a> {
    Run(BoxFuture<'a, crate::Result<Box<dyn Value>>>),
    Invalid(Vec<reze::Issue>),
    JsonOnly,
}

type ErasedMutation<C> =
    Box<dyn for<'a> Fn(&Posted, &'a http::request::Parts, &'a C) -> Decoded<'a> + Send + Sync>;

/// The application: the generated route table, the Rust loaders, guards, and mutations, and the
/// client build.
pub struct App<C> {
    table: &'static Table,
    /// Routes in match order.
    ordered: Vec<&'static Route>,
    loaders: Vec<Option<Erased<C>>>,
    guards: Vec<Option<ErasedGuard<C>>>,
    mutations: Vec<Option<ErasedMutation<C>>>,
    body_limit: usize,
    manifest: Option<Arc<Manifest>>,
    server_timing: bool,
}

/// What a page owns to render its streamed parts after the response has started.
struct Deferred {
    table: &'static Table,
    chain: &'static [usize],
    chunk: Option<Chunk>,
    manifest: Option<Arc<Manifest>>,
    from: usize,
    path: String,
    search: String,
    query: Vec<(String, String)>,
    params: Params,
    data: Vec<Option<Box<dyn Value>>>,
    error: Option<(usize, RouteError)>,
    submission: Option<Submission>,
}

impl Deferred {
    fn page(&self) -> Page<'_> {
        Page {
            table: self.table,
            chain: self.chain,
            path: &self.path,
            search: &self.search,
            query: &self.query,
            params: &self.params,
            data: &self.data,
            error: self.error.clone(),
            submission: self.submission.as_ref(),
        }
    }
}

/// Waits until one of `parts` has the field it waits for, and returns its id.
async fn settled(parts: &[Option<Part>]) -> usize {
    poll_fn(|cx| {
        for (id, part) in parts.iter().enumerate() {
            if let Some(part) = part
                && part.waiting.poll_settle(cx).is_ready()
            {
                return Poll::Ready(id);
            }
        }
        Poll::Pending
    })
    .await
}

/// The body of a page with deferred parts: the document, each part as its field resolves, and
/// the end of the document.
fn streamed(document: String, parts: Vec<Part>, deferred: Deferred) -> Body {
    let parts: Vec<Option<Part>> = parts.into_iter().map(Some).collect();
    let rest = stream::unfold(Some((deferred, parts)), |state| async move {
        let (deferred, mut parts) = state?;
        loop {
            if parts.iter().all(Option::is_none) {
                return Some((Bytes::from_static(b"</body></html>"), None));
            }
            let id = settled(&parts).await;
            let part = parts[id].take().expect("a settled part waits");
            let streamed = render_part(
                &deferred.page(),
                deferred.chunk,
                deferred.manifest.as_deref(),
                deferred.from,
                (id, &part.path),
                parts.len(),
            );
            parts.extend(streamed.parts.into_iter().map(Some));
            if let Some(waiting) = streamed.again {
                parts[id] = Some(Part { path: part.path, waiting });
            } else if let Some(html) = streamed.html {
                return Some((Bytes::from(html), Some((deferred, parts))));
            }
        }
    });
    Body::stream(stream::once(async move { Bytes::from(document) }).chain(rest))
}

/// Wraps the page of a `.call` with `_b` as `{ result, status, segment, html }`.
async fn answer(called: Option<&str>, response: Response<Body>) -> Response<Body> {
    let Some(result) = called else { return response };
    let segment = response
        .headers()
        .get("rz-segment")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("0")
        .to_string();
    let status = response.status().as_u16();
    let html = response.into_body().collect().await;
    let mut body =
        format!("{{\"result\":{result},\"status\":{status},\"segment\":{segment},\"html\":");
    reze::js::push_json_string(&mut body, &String::from_utf8_lossy(&html));
    body.push('}');
    json(200, body)
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

fn response(status: u16, headers: HeaderMap, body: impl Into<Body>) -> Response<Body> {
    let mut response = Response::new(body.into());
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    *response.headers_mut() = headers;
    response
}

fn plain(status: u16, body: &'static str) -> Response<Body> {
    response(status, HeaderMap::new(), body)
}

/// The query of `query` without the names in `skip`, with its `?`.
fn without(query: &[(String, String)], skip: &[&str]) -> String {
    let search = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(query.iter().filter(|(key, _)| !skip.contains(&key.as_str())))
        .finish();
    if search.is_empty() { search } else { format!("?{search}") }
}

fn json(status: u16, body: String) -> Response<Body> {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response(status, headers, body)
}

fn convert(value: serde_json::Value) -> reze::Json {
    use serde_json::Value as V;
    match value {
        V::Null => reze::Json::Null,
        V::Bool(value) => reze::Json::Bool(value),
        V::Number(n) => reze::Json::Number(n.as_f64().unwrap_or(f64::NAN)),
        V::String(text) => reze::Json::String(text),
        V::Array(items) => reze::Json::Array(items.into_iter().map(convert).collect()),
        V::Object(fields) => reze::Json::Object(
            fields.into_iter().map(|(key, value)| (key, convert(value))).collect(),
        ),
    }
}

/// Reads a mutation body: JSON for `.call`, urlencoded form fields otherwise.
fn read_body(
    headers: &HeaderMap,
    body: &Bytes,
    limit: usize,
) -> Result<Posted, (u16, &'static str)> {
    if body.len() > limit {
        return Err((413, "Content Too Large"));
    }
    let kind =
        headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or("");
    if kind.contains("application/json") {
        let value = serde_json::from_slice(body).map_err(|_| (400, "Invalid JSON"))?;
        return Ok(Posted::Json(convert(value)));
    }
    if kind.starts_with("multipart/") {
        return Err((415, "Unsupported Media Type"));
    }
    Ok(Posted::Form(form_urlencoded::parse(body).into_owned().collect()))
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

/// What the loaders of one request write: their data, the shallowest error, the first redirect,
/// and their timings.
struct Run<'r> {
    data: &'r mut [Option<Box<dyn Value>>],
    error: &'r mut Option<(usize, RouteError)>,
    redirect: &'r mut Option<RouteError>,
    timings: &'r mut String,
}

fn redirect(target: &RouteError) -> Response<Body> {
    let mut headers = HeaderMap::new();
    if let Some(location) = target.location.as_deref().and_then(|l| HeaderValue::from_str(l).ok()) {
        headers.insert(header::LOCATION, location);
    }
    response(target.status, headers, Body::empty())
}

/// A redirect from a post: `303` unless it sets another status than the default `307`.
fn posted(target: RouteError) -> RouteError {
    if target.status == 307 { RouteError { status: 303, ..target } } else { target }
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
            guards: table.nodes.iter().map(|_| None).collect(),
            mutations: table.mutations.iter().map(|_| None).collect(),
            body_limit: 1 << 20,
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

    /// Sets the guard of route node `index`, which the generated setters call.
    pub fn set_guard<P, F>(&mut self, index: usize, params: fn(&Params) -> P, guard: F)
    where
        P: Send + 'static,
        F: for<'a> Guard<'a, C, P> + 'static,
    {
        self.guards[index] =
            Some(Box::new(move |raw, request, context| guard.check(params(raw), request, context)));
    }

    /// Sets the mutation `index` of the table, which the generated setters call.
    pub fn set_mutation<I, T, F>(&mut self, index: usize, mutation: F)
    where
        I: reze::Input + Send + 'static,
        T: Value,
        F: for<'a> Mutation<'a, C, I, T> + 'static,
    {
        self.mutations[index] = Some(Box::new(move |body, request, context| {
            let mut issues = Vec::new();
            let input = match body {
                Posted::Json(json) => I::json(Some(json), &[], &mut issues),
                Posted::Form(_) if !I::FORM => return Decoded::JsonOnly,
                Posted::Form(form) => I::form(form, &mut issues),
            };
            match input {
                Some(input) if issues.is_empty() => {
                    let future = mutation.run(input, request, context);
                    Decoded::Run(Box::pin(async move {
                        future.await.map(|value| Box::new(value) as Box<dyn Value>)
                    }))
                }
                _ => Decoded::Invalid(issues),
            }
        }));
    }

    /// The largest mutation body in bytes, 1 MB by default: the adapters answer `413` above it.
    pub fn body_limit(&self) -> usize {
        self.body_limit
    }

    pub fn set_body_limit(&mut self, bytes: usize) {
        self.body_limit = bytes;
    }

    pub fn set_manifest(&mut self, manifest: Manifest) {
        self.manifest = Some(Arc::new(manifest));
    }

    /// Writes loader and render durations into `Server-Timing`: on with the `dev` feature, and
    /// opt-in otherwise, because it reveals timing to every client.
    pub fn set_server_timing(&mut self, on: bool) {
        self.server_timing = on;
    }

    /// The `Link` values of `103 Early Hints` for `request`: the dispatcher as `modulepreload`,
    /// for a document request. A server that can write a `103` writes them before it calls
    /// [`App::handle`]. The document response carries them in `Link`, which a CDN can turn into
    /// a `103` for later requests.
    pub fn hints<B>(&self, request: &Request<B>) -> Vec<String> {
        self.links(request.method(), request.uri())
    }

    fn links(&self, method: &Method, uri: &http::Uri) -> Vec<String> {
        let document = (method == Method::GET || method == Method::HEAD)
            && !form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
                .any(|(key, _)| key == "_b");
        match &self.manifest {
            Some(manifest) if document => {
                vec![format!("<{}>; rel=modulepreload", manifest.dispatcher)]
            }
            _ => Vec::new(),
        }
    }

    /// Answers one request, whose body a mutation reads.
    pub async fn handle(&self, request: Request<Bytes>, context: &C) -> Response<Body> {
        let (parts, body) = request.into_parts();
        let span = info_span!(
            "request",
            method = %parts.method,
            path = parts.uri.path(),
            status = field::Empty
        );
        let response = self.respond(&parts, &body, context).instrument(span.clone()).await;
        span.record("status", response.status().as_u16());
        response
    }

    async fn respond(
        &self,
        request: &http::request::Parts,
        body: &Bytes,
        context: &C,
    ) -> Response<Body> {
        let path = request.uri.path();
        let query: Vec<(String, String)> =
            form_urlencoded::parse(request.uri.query().unwrap_or("").as_bytes())
                .into_owned()
                .collect();
        #[cfg(feature = "dev")]
        if path == "/__reze/load" {
            return self.load_json(&query, context).await;
        }
        #[cfg(feature = "dev")]
        if path == "/__reze/guard" {
            return self.guard_check(request, &query, context).await;
        }
        #[cfg(feature = "dev")]
        if path == "/__reze/mutate" {
            return self.mutate_json(request, body, &query, context).await;
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
        let mut search = without(&query, &["_b", "_s"]);
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
                return response(308, headers, Body::empty());
            }
        }
        let prerender = request.headers.contains_key("rz-prerender");
        let mut timings = String::new();
        let Some((route, params, _)) = found else {
            let deferred = Deferred {
                table: self.table,
                chain: self.table.root,
                chunk: None,
                manifest: self.manifest.clone(),
                from: 0,
                path: path.to_string(),
                search,
                query,
                params: Params::new(),
                data: Vec::new(),
                error: Some((self.table.root.len(), RouteError::not_found())),
                submission: None,
            };
            let rendered = self.render(&deferred, prerender, &mut timings).await;
            return self.finish(rendered, HeaderMap::new(), &timings, request, deferred);
        };
        let chain = route.chain;
        let mut from =
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
        if method == Method::POST && cross_site(&request.headers) {
            return plain(403, "Forbidden");
        }
        let post = method == Method::POST;
        let to = |failure: RouteError| if post { posted(failure) } else { failure };

        let mut guarded = None;
        for (depth, &node) in chain.iter().enumerate() {
            if !self.table.nodes[node].guard {
                continue;
            }
            let span = info_span!("guard", route = route.pattern, depth);
            let result = match self.guards[node].as_ref() {
                Some(guard) => guard(&params, request, context).instrument(span).await,
                None => {
                    tracing::error!(file = self.table.nodes[node].file, "no Rust guard is set");
                    Err(RouteError::internal())
                }
            };
            match result {
                Ok(()) => {}
                Err(failure) if failure.location.is_some() => return redirect(&to(failure)),
                Err(failure) => {
                    guarded = Some((depth, failure));
                    from = 0;
                    break;
                }
            }
        }

        let mut submission = None;
        let mut called = None;
        let mut status = 200;
        if post && guarded.is_none() {
            let id = get("_m").unwrap_or_default();
            let Some(index) = self.table.mutations.iter().position(|m| m.id == id) else {
                return plain(404, "Not Found");
            };
            let def = &self.table.mutations[index];
            search = without(&query, &["_b", "_s", "_m"]);
            let input = match read_body(&request.headers, body, self.body_limit) {
                Ok(input) => input,
                Err((status, message)) => return plain(status, message),
            };
            let Some(mutation) = self.mutations[index].as_ref() else {
                tracing::error!(export = def.export, file = def.file, "no Rust mutation is set");
                return plain(500, "Internal Server Error");
            };
            let span = info_span!("mutation", export = def.export, file = def.file);
            match (mutation(&input, request, context), input) {
                (Decoded::JsonOnly, _) => return plain(415, "This mutation takes JSON input"),
                (Decoded::Invalid(issues), Posted::Json(_)) => {
                    return json(422, format!("{{\"issues\":{}}}", reze::to_json(&issues)));
                }
                (Decoded::Invalid(issues), Posted::Form(form)) => {
                    submission = Some(Submission::new(def.id, &form, issues));
                    status = 422;
                }
                (Decoded::Run(future), input) => match future.instrument(span).await {
                    Err(failure) if failure.location.is_some() => {
                        return redirect(&posted(failure));
                    }
                    Err(failure) if matches!(input, Posted::Json(_)) => {
                        let mut message = String::from("{\"message\":");
                        reze::js::push_json_string(&mut message, &failure.message);
                        message.push('}');
                        return json(failure.status, message);
                    }
                    Err(failure) => guarded = Some((chain.len() - 1, failure)),
                    Ok(value) if build.is_none() => {
                        if matches!(input, Posted::Json(_)) {
                            return json(200, format!("{{\"result\":{}}}", value.json()));
                        }
                        let mut headers = HeaderMap::new();
                        if let Ok(location) = HeaderValue::from_str(&format!("{path}{search}")) {
                            headers.insert(header::LOCATION, location);
                        }
                        return response(303, headers, Body::empty());
                    }
                    Ok(value) => {
                        if matches!(input, Posted::Json(_)) {
                            called = Some(value.json());
                        }
                        if let Some(patterns) = def.revalidate {
                            from = chain
                                .iter()
                                .position(|&node| {
                                    patterns.contains(&self.table.nodes[node].pattern)
                                })
                                .unwrap_or(chain.len());
                        }
                    }
                },
            }
        }
        let until = guarded.as_ref().map_or(chain.len(), |(depth, _)| *depth);
        let called = called.as_deref();

        let mut data: Vec<Option<Box<dyn Value>>> = chain.iter().map(|_| None).collect();
        let (mut error, mut stop) = (guarded, None);
        let mut state =
            Run { data: &mut data, error: &mut error, redirect: &mut stop, timings: &mut timings };
        self.run(route, (from..until).collect(), &params, context, &mut state).await;
        if let Some(target) = &stop {
            return redirect(target);
        }
        let mut headers = HeaderMap::new();
        if !post
            && let Some(seconds) =
                chain.iter().filter_map(|&node| self.table.nodes[node].revalidate).min()
            && let Ok(value) =
                HeaderValue::from_str(&format!("s-maxage={seconds}, stale-while-revalidate"))
        {
            headers.insert(header::CACHE_CONTROL, value);
        }
        if from == chain.len() {
            headers
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
            headers.insert("rz-segment", HeaderValue::from_static("-1"));
            return answer(called, response(200, headers, Body::empty())).await;
        }
        let mut deferred = Deferred {
            table: self.table,
            chain,
            chunk: route.chunk,
            manifest: self.manifest.clone(),
            from,
            path: path.to_string(),
            search,
            query,
            params,
            data,
            error,
            submission,
        };
        if from > 0 {
            let rendered = self.render(&deferred, prerender, &mut timings).await;
            if rendered.is_ok() {
                headers.insert("rz-segment", HeaderValue::from(from));
                let response = self.finish(rendered, headers, &timings, request, deferred);
                return answer(called, response).await;
            }
            let mut state = Run {
                data: &mut deferred.data,
                error: &mut deferred.error,
                redirect: &mut stop,
                timings: &mut timings,
            };
            self.run(route, (0..from).collect(), &deferred.params, context, &mut state).await;
            if let Some(target) = &stop {
                return redirect(target);
            }
            deferred.from = 0;
        }
        let rendered = self.render(&deferred, prerender, &mut timings).await.map(|mut rendered| {
            if rendered.status == 200 {
                rendered.status = status;
            }
            rendered
        });
        answer(called, self.finish(rendered, headers, &timings, request, deferred)).await
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
                Some(Err(failure)) if failure.location.is_some() => {
                    state.redirect.get_or_insert(failure);
                }
                Some(Err(failure)) if state.error.as_ref().is_none_or(|(at, _)| at > depth) => {
                    *state.error = Some((*depth, failure));
                }
                Some(Err(_)) => {}
            }
        }
    }

    /// Renders the page of `deferred`, and for a prerender waits for its deferred fields and
    /// renders again until it streams nothing.
    async fn render(
        &self,
        deferred: &Deferred,
        prerender: bool,
        timings: &mut String,
    ) -> crate::Result<Rendered> {
        loop {
            let span = info_span!("render", bytes = field::Empty);
            let start = Instant::now();
            let page = deferred.page();
            let rendered = span.in_scope(|| {
                render(&page, deferred.chunk, deferred.manifest.as_deref(), deferred.from)
            });
            if self.server_timing {
                timing(timings, "render", start.elapsed().as_secs_f64());
            }
            if let Ok(rendered) = &rendered {
                span.record("bytes", rendered.document.len());
                if prerender && !rendered.parts.is_empty() {
                    let parts = &rendered.parts;
                    join_all(parts.iter().map(|part| poll_fn(|cx| part.waiting.poll_settle(cx))))
                        .await;
                    continue;
                }
            }
            return rendered;
        }
    }

    fn finish(
        &self,
        rendered: crate::Result<Rendered>,
        mut headers: HeaderMap,
        timings: &str,
        request: &http::request::Parts,
        deferred: Deferred,
    ) -> Response<Body> {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
        for link in self.links(&request.method, &request.uri) {
            if let Ok(value) = HeaderValue::from_str(&link) {
                headers.append(header::LINK, value);
            }
        }
        if !timings.is_empty()
            && let Ok(value) = HeaderValue::from_str(timings)
        {
            headers.insert("server-timing", value);
        }
        let head = request.method == Method::HEAD;
        let Rendered { document, status, parts } = match rendered {
            Ok(rendered) => rendered,
            Err(error) => {
                Rendered { document: built_in(&error), status: error.status, parts: Vec::new() }
            }
        };
        if !parts.is_empty() {
            let body = if head { Body::empty() } else { streamed(document, parts, deferred) };
            return response(status, headers, body);
        }
        if status == 200 && request.method != Method::POST {
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
                return response(304, headers, Body::empty());
            }
        }
        response(status, headers, if head { Body::empty() } else { Body::from(document) })
    }

    /// The `dev` endpoint `GET /__reze/guard?m=<file>&u=<url>&<params>`: runs one guard over the
    /// request's headers and the page's URL, for the JS dev server, and answers `204` when it
    /// passes.
    #[cfg(feature = "dev")]
    async fn guard_check(
        &self,
        request: &http::request::Parts,
        query: &[(String, String)],
        context: &C,
    ) -> Response<Body> {
        let (mut file, mut url) = (None, "/");
        let mut params = Params::new();
        for (name, value) in query {
            match name.as_str() {
                "m" => file = Some(value.as_str()),
                "u" => url = value,
                _ => params.insert(name.clone(), value.clone()),
            }
        }
        let node = self.table.nodes.iter().position(|node| Some(node.file) == file);
        let Some(guard) = node.and_then(|node| self.guards[node].as_ref()) else {
            return plain(404, "Not Found");
        };
        let Ok(page) = Request::get(url).body(()) else { return plain(400, "Bad Request") };
        let (mut page, _) = page.into_parts();
        page.headers = request.headers.clone();
        match guard(&params, &page, context).await {
            Ok(()) => response(204, HeaderMap::new(), Body::empty()),
            Err(error) if error.location.is_some() => redirect(&error),
            Err(error) => response(error.status, HeaderMap::new(), error.message),
        }
    }

    /// The `dev` endpoint `POST /__reze/mutate?m=<id>`: runs one mutation over the request's
    /// headers and the input the JS dev server checked, and answers its result as JSON.
    #[cfg(feature = "dev")]
    async fn mutate_json(
        &self,
        request: &http::request::Parts,
        body: &Bytes,
        query: &[(String, String)],
        context: &C,
    ) -> Response<Body> {
        let id = query.iter().find(|(name, _)| name == "m").map(|(_, value)| value.as_str());
        let index = self.table.mutations.iter().position(|m| Some(m.id) == id);
        let Some(mutation) = index.and_then(|index| self.mutations[index].as_ref()) else {
            return plain(404, "Not Found");
        };
        let input = match read_body(&request.headers, body, self.body_limit) {
            Ok(input) => input,
            Err((status, message)) => return plain(status, message),
        };
        match mutation(&input, request, context) {
            Decoded::JsonOnly => plain(415, "This mutation takes JSON input"),
            Decoded::Invalid(issues) => {
                json(422, format!("{{\"issues\":{}}}", reze::to_json(&issues)))
            }
            Decoded::Run(future) => match future.await {
                Ok(value) => json(200, format!("{{\"result\":{}}}", value.json())),
                Err(error) if error.location.is_some() => redirect(&posted(error)),
                Err(error) => response(error.status, HeaderMap::new(), error.message),
            },
        }
    }

    /// The `dev` endpoint `GET /__reze/load?m=<file>&<params>`: the data of one loader as JSON,
    /// for the JS dev server.
    #[cfg(feature = "dev")]
    async fn load_json(&self, query: &[(String, String)], context: &C) -> Response<Body> {
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
            Ok(value) => loop {
                reze::defer::take_waiting();
                let json = value.json();
                let waiting = reze::defer::take_waiting();
                if waiting.is_empty() {
                    let mut headers = HeaderMap::new();
                    headers
                        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
                    break response(200, headers, json);
                }
                join_all(waiting.iter().map(|field| poll_fn(|cx| field.poll_settle(cx)))).await;
            },
            Err(error) if error.location.is_some() => redirect(&error),
            Err(error) => response(error.status, HeaderMap::new(), error.message),
        }
    }
}
