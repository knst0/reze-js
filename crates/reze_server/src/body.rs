use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::stream::{BoxStream, Stream, StreamExt};

/// A response body: whole, or a page that streams its deferred parts after the document.
pub struct Body(Inner);

enum Inner {
    Full(Option<Bytes>),
    Stream(BoxStream<'static, Bytes>),
}

impl Body {
    pub fn empty() -> Self {
        Self(Inner::Full(None))
    }

    pub(crate) fn stream(stream: impl Stream<Item = Bytes> + Send + 'static) -> Self {
        Self(Inner::Stream(stream.boxed()))
    }

    /// The whole body, or the body back when it streams.
    pub fn into_bytes(self) -> Result<Bytes, Self> {
        match self.0 {
            Inner::Full(bytes) => Ok(bytes.unwrap_or_default()),
            stream => Err(Self(stream)),
        }
    }

    /// Waits for every chunk and joins them.
    pub async fn collect(self) -> Bytes {
        match self.0 {
            Inner::Full(bytes) => bytes.unwrap_or_default(),
            Inner::Stream(stream) => {
                let chunks: Vec<Bytes> = stream.collect().await;
                chunks.concat().into()
            }
        }
    }

    /// The length of a whole body.
    pub fn len(&self) -> Option<usize> {
        match &self.0 {
            Inner::Full(bytes) => Some(bytes.as_ref().map_or(0, Bytes::len)),
            Inner::Stream(_) => None,
        }
    }

    /// Whether no chunk is left.
    pub fn is_empty(&self) -> bool {
        matches!(&self.0, Inner::Full(None))
    }
}

impl From<Bytes> for Body {
    fn from(bytes: Bytes) -> Self {
        Self(Inner::Full(Some(bytes).filter(|bytes| !bytes.is_empty())))
    }
}

impl From<String> for Body {
    fn from(text: String) -> Self {
        Bytes::from(text).into()
    }
}

impl From<&'static str> for Body {
    fn from(text: &'static str) -> Self {
        Bytes::from_static(text.as_bytes()).into()
    }
}

impl Stream for Body {
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Bytes>> {
        match &mut self.0 {
            Inner::Full(bytes) => Poll::Ready(bytes.take()),
            Inner::Stream(stream) => stream.poll_next_unpin(cx),
        }
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Inner::Full(bytes) => f.debug_tuple("Body").field(bytes).finish(),
            Inner::Stream(_) => f.write_str("Body(<stream>)"),
        }
    }
}
