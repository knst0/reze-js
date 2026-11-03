//! The `tower` adapter, for axum, hyper, poem, and salvo.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};

use crate::App;

/// A `tower::Service` that answers every request with `app`, given the application's context.
pub fn service<C: Send + Sync + 'static>(app: impl Into<App<C>>, context: C) -> Service<C> {
    Service { app: Arc::new(app.into()), context: Arc::new(context) }
}

pub struct Service<C> {
    app: Arc<App<C>>,
    context: Arc<C>,
}

impl<C> Clone for Service<C> {
    fn clone(&self) -> Self {
        Self { app: self.app.clone(), context: self.context.clone() }
    }
}

impl<C, B> tower_service::Service<Request<B>> for Service<C>
where
    C: Send + Sync + 'static,
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    type Response = Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let (app, context) = (self.app.clone(), self.context.clone());
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let body = match Limited::new(body, app.body_limit()).collect().await {
                Ok(body) => body.to_bytes(),
                Err(error) => {
                    let too_large = error.downcast_ref::<LengthLimitError>().is_some();
                    let mut response = Response::new(Full::new(Bytes::new()));
                    *response.status_mut() = if too_large {
                        StatusCode::PAYLOAD_TOO_LARGE
                    } else {
                        StatusCode::BAD_REQUEST
                    };
                    return Ok(response);
                }
            };
            let request = Request::from_parts(parts, body);
            Ok(app.handle(request, &context).await.map(Full::new))
        })
    }
}
