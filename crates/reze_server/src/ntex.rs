//! The `ntex` adapter, and the server that the generated `main` runs.

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use futures_util::StreamExt;
use http::Request;
use ntex::http::StatusCode;
use ntex::util::BytesMut;
use ntex::web::types::Payload;
use ntex::web::{self, HttpRequest, HttpResponse, HttpServer};

use crate::App;

/// Serves `app` on `HOST` (default `0.0.0.0`) and `PORT` (default `3000`), with the context that
/// `context` resolves to, and the client build in `../client` beside the executable. Prints
/// `reze: listening on <origin>` once it accepts connections.
pub fn run<C, F>(app: App<C>, context: F) -> io::Result<()>
where
    C: Send + Sync + 'static,
    F: Future<Output = C> + 'static,
{
    ntex::rt::System::build().name("reze").build(ntex::rt::DefaultRuntime).block_on(async move {
        let context = Arc::new(context.await);
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port = std::env::var("PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(3000u16);
        let listener = TcpListener::bind((host.as_str(), port))?;
        let address = listener.local_addr()?;
        let client = client_dir();
        let app = Arc::new(app);
        let server = HttpServer::new(async move |_| {
            let (app, context, client) = (app.clone(), context.clone(), client.clone());
            web::App::new().default_service(web::route().to(
                async move |request: HttpRequest, payload: Payload| {
                    serve(&app, &context, client.as_deref(), &request, payload).await
                },
            ))
        })
        .listen(listener, ntex::SharedCfg::new("reze"))?
        .run();
        println!("reze: listening on http://{address}");
        server.await
    })
}

fn client_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?.join("client");
    dir.is_dir().then_some(dir)
}

async fn serve<C: Send + Sync + 'static>(
    app: &App<C>,
    context: &C,
    client: Option<&Path>,
    request: &HttpRequest,
    payload: Payload,
) -> HttpResponse {
    if let Some(response) = client.and_then(|client| asset(client, request)) {
        return response;
    }
    match read(payload, app.body_limit()).await {
        Ok(body) => respond(app, request, body, context).await,
        Err(status) => HttpResponse::new(status),
    }
}

async fn read(mut payload: Payload, limit: usize) -> Result<Bytes, StatusCode> {
    let mut body = BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
        if body.len() + chunk.len() > limit {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::copy_from_slice(&body))
}

fn asset(client: &Path, request: &HttpRequest) -> Option<HttpResponse> {
    if request.method() != ntex::http::Method::GET && request.method() != ntex::http::Method::HEAD {
        return None;
    }
    let path = percent_encoding::percent_decode_str(request.path()).decode_utf8().ok()?;
    let relative = path.strip_prefix('/')?;
    let extension = Path::new(relative).extension()?.to_str()?;
    if relative.split('/').any(|segment| segment.is_empty() || segment.starts_with('.'))
        || relative.contains('\\')
    {
        return None;
    }
    let bytes = std::fs::read(client.join(relative)).ok()?;
    let mut response = HttpResponse::Ok();
    response.content_type(content_type(extension));
    if relative.starts_with("assets/") {
        response.header("cache-control", "public, max-age=31536000, immutable");
    }
    Some(response.body(bytes))
}

fn content_type(extension: &str) -> &'static str {
    match extension {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

async fn respond<C: Send + Sync + 'static>(
    app: &App<C>,
    request: &HttpRequest,
    body: Bytes,
    context: &C,
) -> HttpResponse {
    let mut builder =
        Request::builder().method(request.method().as_str()).uri(request.uri().to_string());
    for (name, value) in request.headers() {
        builder = builder.header(name.as_str(), value.as_bytes());
    }
    let Ok(request) = builder.body(body) else {
        return HttpResponse::new(StatusCode::BAD_REQUEST);
    };
    let (parts, body) = app.handle(request, context).await.into_parts();
    let mut response = HttpResponse::builder(
        StatusCode::from_u16(parts.status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    );
    for (name, value) in &parts.headers {
        response.header(name.as_str(), value.as_bytes());
    }
    match body.into_bytes() {
        Ok(bytes) => response.body(ntex::util::Bytes::copy_from_slice(&bytes)),
        Err(body) => response.streaming(
            body.map(|chunk| Ok::<_, Infallible>(ntex::util::Bytes::copy_from_slice(&chunk))),
        ),
    }
}
