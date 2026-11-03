//! The `tower` adapter, for axum, hyper, poem, and salvo.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;

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

impl<C: Send + Sync + 'static, B: Send + 'static> tower_service::Service<Request<B>>
    for Service<C>
{
    type Response = Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let (app, context) = (self.app.clone(), self.context.clone());
        Box::pin(async move { Ok(app.handle(request, &context).await.map(Full::new)) })
    }
}
