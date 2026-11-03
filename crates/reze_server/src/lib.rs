//! The Rust server of reze-js: renders the routes `reze_build` generates, with the data of Rust
//! loaders. The core is sans-io: [`App::handle`] takes a parsed request and a context and returns a
//! response, on any executor and behind any HTTP framework.

#[cfg(feature = "actix")]
pub mod actix;
mod app;
mod html;
mod render;
#[cfg(feature = "tower")]
pub mod tower;

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::Path;

use futures_util::future::BoxFuture;
use serde::Deserialize;

pub use {http, reze};

pub use app::App;
pub use html::Html;
pub use render::{Mark, Page, Render, children, push_segment};

/// A route error: its status and message reach the nearest `<Errored>`, or the built-in error page.
/// A redirect stops the request instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteError {
    pub status: u16,
    pub message: String,
    /// The target of a redirect.
    pub location: Option<String>,
}

impl RouteError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self { status, message: message.into(), location: None }
    }

    /// A `307` redirect to `location`; set `status` for `301`, `302`, `303`, or `308`.
    pub fn redirect(location: impl Into<String>) -> Self {
        Self { status: 307, message: String::new(), location: Some(location.into()) }
    }

    /// `404 Not Found`.
    pub fn not_found() -> Self {
        Self::new(404, "Not Found")
    }

    /// `500`, with a generic message: the real one belongs in the server's logs.
    pub fn internal() -> Self {
        Self::new(500, "Internal Server Error")
    }
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.status, self.message)
    }
}

impl std::error::Error for RouteError {}

pub type Result<T, E = RouteError> = std::result::Result<T, E>;

/// The params of the matched route, decoded, by name.
#[derive(Clone, Debug, Default)]
pub struct Params(Vec<(String, String)>);

impl Params {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

/// One segment of a route pattern.
#[derive(Clone, Copy, Debug)]
pub enum Segment {
    Static(&'static str),
    Param(&'static str),
    Optional(&'static str),
    Splat(&'static str),
}

impl Segment {
    fn rank(self) -> u8 {
        match self {
            Segment::Static(_) => 0,
            Segment::Param(_) => 1,
            Segment::Optional(_) => 2,
            Segment::Splat(_) => 3,
        }
    }
}

/// Renders one route level into `out`.
pub type RenderFn = fn(&mut Html, &mut Render, &Page<'_>) -> Result<()>;

/// A route file: a layout or a page.
pub struct Node {
    pub file: &'static str,
    pub render: RenderFn,
    /// Whether the file has a `loader.external<T>()` the application sets.
    pub loader: bool,
    /// Whether the file has a `guardRoute.external()` the application sets.
    pub guard: bool,
    pub revalidate: Option<u32>,
}

/// A page with its layouts.
pub struct Route {
    pub pattern: &'static str,
    pub segments: &'static [Segment],
    /// The nodes from the root layout down to the page.
    pub chain: &'static [usize],
    /// The client chunks, when the page has client code.
    pub chunk: Option<Chunk>,
}

/// The client code of a page, by the keys of [`Manifest::chunks`].
#[derive(Clone, Copy)]
pub enum Chunk {
    /// One chunk with every factory of the route.
    Route(&'static str),
    /// A chunk for each factory id.
    Factories(&'static [(u32, &'static str)]),
}

/// The generated route table.
pub struct Table {
    pub nodes: &'static [Node],
    pub routes: &'static [Route],
    /// The root layout, which renders the `404` of an unmatched URL.
    pub root: &'static [usize],
}

/// Loader data of any type, which the dev server also answers as JSON.
pub trait Value: Any + Send + Sync {
    fn any(&self) -> &dyn Any;
    fn json(&self) -> String;
}

impl<T: reze::Data + Any + Send + Sync> Value for T {
    fn any(&self) -> &dyn Any {
        self
    }

    fn json(&self) -> String {
        reze::to_json(self)
    }
}

/// A Rust loader: `async fn(Params, &C) -> Result<T>`.
pub trait Loader<'a, C: 'a, P, T>: Send + Sync {
    fn load(&self, params: P, context: &'a C) -> BoxFuture<'a, Result<T>>;
}

impl<'a, C, P, T, F, Fut> Loader<'a, C, P, T> for F
where
    C: 'a,
    F: Fn(P, &'a C) -> Fut + Send + Sync,
    Fut: Future<Output = Result<T>> + Send + 'a,
{
    fn load(&self, params: P, context: &'a C) -> BoxFuture<'a, Result<T>> {
        Box::pin(self(params, context))
    }
}

/// A Rust guard: `async fn(Params, &http::request::Parts, &C) -> Result<()>`. It sees the
/// request's URL and headers, and stops the request with a `RouteError` or a redirect.
pub trait Guard<'a, C: 'a, P>: Send + Sync {
    fn check(
        &self,
        params: P,
        request: &'a http::request::Parts,
        context: &'a C,
    ) -> BoxFuture<'a, Result<()>>;
}

impl<'a, C, P, F, Fut> Guard<'a, C, P> for F
where
    C: 'a,
    F: Fn(P, &'a http::request::Parts, &'a C) -> Fut + Send + Sync,
    Fut: Future<Output = Result<()>> + Send + 'a,
{
    fn check(
        &self,
        params: P,
        request: &'a http::request::Parts,
        context: &'a C,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(self(params, request, context))
    }
}

/// The URLs of the client build, which `vite build` writes to `dist/server/reze.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    /// The id of the build, which client requests carry in `_b`.
    pub build: String,
    pub dispatcher: String,
    pub stream: String,
    /// The URL of each client chunk, by its key.
    pub chunks: HashMap<String, String>,
}

impl Manifest {
    pub fn parse(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self::parse(&std::fs::read_to_string(path)?)?)
    }
}
