//! Deferred fields: loader data that streams into its `<Loading>` boundary when it resolves.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};

use crate::Data;
use crate::input::{Input, Issue, Json, Key};

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A loader field that the server sends after the document: the page renders the fallback of the
/// `<Loading>` boundary that reads it, then streams the boundary once the future resolves.
pub struct Defer<T>(Arc<Slot<T>>);

struct Slot<T> {
    value: OnceLock<T>,
    future: Mutex<Option<Pending<T>>>,
}

/// A deferred field the response waits for.
pub trait Settle: Send + Sync {
    /// Polls the field's future, and is ready once the field has its value.
    fn poll_settle(&self, cx: &mut Context<'_>) -> Poll<()>;
}

impl<T: Send + Sync> Settle for Slot<T> {
    fn poll_settle(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.value.get().is_some() {
            return Poll::Ready(());
        }
        let mut future = self.future.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(pending) = future.as_mut() else { return Poll::Ready(()) };
        match pending.as_mut().poll(cx) {
            Poll::Ready(value) => {
                *future = None;
                let _ = self.value.set(value);
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

thread_local! {
    static WAITING: RefCell<Vec<Arc<dyn Settle>>> = const { RefCell::new(Vec::new()) };
}

/// Takes the deferred fields that writes and renders on this thread found without a value.
#[doc(hidden)]
pub fn take_waiting() -> Vec<Arc<dyn Settle>> {
    WAITING.with(|waiting| std::mem::take(&mut *waiting.borrow_mut()))
}

impl<T: Send + Sync + 'static> Defer<T> {
    /// A field whose value `future` produces after the document is sent.
    pub fn new(future: impl Future<Output = T> + Send + 'static) -> Self {
        Self(Arc::new(Slot { value: OnceLock::new(), future: Mutex::new(Some(Box::pin(future))) }))
    }

    /// A field that already has its value, which renders in place.
    pub fn ready(value: T) -> Self {
        Self(Arc::new(Slot { value: OnceLock::from(value), future: Mutex::new(None) }))
    }

    /// The value, once the future has resolved.
    pub fn get(&self) -> Option<&T> {
        self.0.value.get()
    }

    /// Records that a read found the field without its value.
    #[doc(hidden)]
    pub fn wait(&self) {
        let slot: Arc<dyn Settle> = self.0.clone();
        WAITING.with(|waiting| waiting.borrow_mut().push(slot));
    }
}

impl<T> Clone for Defer<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: fmt::Debug> fmt::Debug for Defer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.value.get() {
            Some(value) => f.debug_tuple("Defer").field(value).finish(),
            None => f.write_str("Defer(<pending>)"),
        }
    }
}

/// A field without its value writes `null` and is recorded, so the writer can wait and write again.
impl<T: Data + Send + Sync + 'static> Data for Defer<T> {
    fn write(&self, out: &mut String) {
        match self.get() {
            Some(value) => value.write(out),
            None => {
                self.wait();
                out.push_str("null");
            }
        }
    }

    fn wire(&self, out: &mut String) {
        match self.get() {
            Some(value) => value.wire(out),
            None => {
                self.wait();
                out.push_str("null");
            }
        }
    }
}

/// A deferred field is loader output only: as mutation input it is always an issue.
impl<T> Input for Defer<T> {
    fn describe() -> String {
        "no value".into()
    }

    fn json(_value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        issues.push(Issue { message: "Expected no value".into(), path: path.to_vec() });
        None
    }
}
