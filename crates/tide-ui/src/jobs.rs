//! Where background work runs.
//!
//! The app hands every background job to [`Jobs`]. In production that is the tokio runtime.
//! Tests use a manual executor: work queues on a private runtime that only advances when the
//! test settles the app, so tests drive the same spawn → event → `handle_background` path as
//! production, in a deterministic order, instead of a synchronous fallback.
use std::future::Future;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct Jobs {
    kind: Kind,
}

#[derive(Clone)]
enum Kind {
    Runtime(tokio::runtime::Handle),
    #[cfg(test)]
    Manual(std::sync::Arc<manual::Manual>),
}

/// Keeps a job counted as in flight until it is dropped.
pub(crate) struct Tracked {
    #[cfg(test)]
    _count: Option<manual::Count>,
}

impl Jobs {
    /// Runs background work on `handle`.
    #[must_use]
    pub fn runtime(handle: tokio::runtime::Handle) -> Self {
        Self {
            kind: Kind::Runtime(handle),
        }
    }

    fn handle(&self) -> &tokio::runtime::Handle {
        match &self.kind {
            Kind::Runtime(handle) => handle,
            #[cfg(test)]
            Kind::Manual(manual) => manual.runtime.handle(),
        }
    }

    /// Marks work that must finish before the app has settled.
    pub(crate) fn track(&self) -> Tracked {
        Tracked {
            #[cfg(test)]
            _count: match &self.kind {
                Kind::Runtime(_) => None,
                Kind::Manual(manual) => Some(manual.count()),
            },
        }
    }

    /// Runs a future to completion in the background.
    pub(crate) fn spawn<T: Send + 'static>(
        &self,
        future: impl Future<Output = T> + Send + 'static,
    ) -> JoinHandle<T> {
        let tracked = self.track();
        self.handle().spawn(async move {
            let _tracked = tracked;
            future.await
        })
    }

    /// Runs work that lives as long as its owner, such as a worker loop.
    pub(crate) fn spawn_daemon(
        &self,
        future: impl Future<Output = ()> + Send + 'static,
    ) -> JoinHandle<()> {
        self.handle().spawn(future)
    }

    /// Runs blocking work off the UI thread.
    pub(crate) fn spawn_blocking(&self, work: impl FnOnce() + Send + 'static) -> JoinHandle<()> {
        let tracked = self.track();
        match &self.kind {
            Kind::Runtime(handle) => handle.spawn_blocking(move || {
                let _tracked = tracked;
                work();
            }),
            // Queue the start too, so nothing runs before the test settles.
            #[cfg(test)]
            Kind::Manual(manual) => manual.runtime.spawn(async move {
                let _tracked = tracked;
                let _ = tokio::task::spawn_blocking(work).await;
            }),
        }
    }
}

#[cfg(test)]
mod manual {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) struct Manual {
        pub(crate) runtime: tokio::runtime::Runtime,
        in_flight: Arc<AtomicUsize>,
    }

    pub(crate) struct Count(Arc<AtomicUsize>);

    impl Drop for Count {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    impl Manual {
        pub(crate) fn count(&self) -> Count {
            self.in_flight.fetch_add(1, Ordering::AcqRel);
            Count(self.in_flight.clone())
        }
    }

    impl super::Jobs {
        /// A test executor that runs queued work only while [`Self::advance`] drives it.
        pub(crate) fn manual() -> Self {
            Self {
                kind: super::Kind::Manual(Arc::new(Manual {
                    runtime: tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("test runtime"),
                    in_flight: Arc::new(AtomicUsize::new(0)),
                })),
            }
        }

        /// Lets queued work run briefly; true while tracked work is still in flight.
        pub(crate) fn advance(&self) -> bool {
            let super::Kind::Manual(manual) = &self.kind else {
                return false;
            };
            manual
                .runtime
                .block_on(async { tokio::time::sleep(std::time::Duration::from_millis(1)).await });
            manual.in_flight.load(Ordering::Acquire) > 0
        }
    }
}
