//! The `actix` adapter, for actix-web.

use std::sync::Arc;

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, Route, web};
use bytes::Bytes;
use http::Request;

use crate::App;

/// A route for `default_service` that answers every request with `app`, given the application's
/// context from its `web::Data<C>`.
pub fn service<C: Send + Sync + 'static>(app: Arc<App<C>>) -> Route {
    web::to(move |request: HttpRequest, body: web::Payload, context: web::Data<C>| {
        let app = app.clone();
        async move {
            match body.to_bytes_limited(app.body_limit()).await {
                Ok(Ok(body)) => respond(&app, &request, body, &context).await,
                Ok(Err(_)) => HttpResponse::BadRequest().finish(),
                Err(_) => HttpResponse::PayloadTooLarge().finish(),
            }
        }
    })
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
        return HttpResponse::BadRequest().finish();
    };
    let (parts, body) = app.handle(request, context).await.into_parts();
    let mut response = HttpResponse::build(
        StatusCode::from_u16(parts.status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    );
    for (name, value) in &parts.headers {
        response.append_header((name.as_str(), value.as_bytes()));
    }
    response.body(body)
}
