//! Retain the listener's existing connection slot through each actual worker.
use std::future::Future;
use std::sync::Arc;
use tokio::sync::OwnedSemaphorePermit;

tokio::task_local! {
    static CONNECTION_LEASE: Arc<OwnedSemaphorePermit>;
}

pub(crate) async fn with_connection_lease<T>(
    permit: OwnedSemaphorePermit,
    connection: impl Future<Output = T>,
) -> T {
    CONNECTION_LEASE.scope(Arc::new(permit), connection).await
}

fn current() -> Option<Arc<OwnedSemaphorePermit>> {
    CONNECTION_LEASE.try_with(Arc::clone).ok()
}

pub(super) fn spawn_owned_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    // Capture on the async connection, never by reading task-local state on
    // the blocking thread. Lexical ownership covers unwinding as well.
    let lease = current();
    tokio::task::spawn_blocking(move || {
        let output = operation();
        drop(lease);
        output
    })
}

#[cfg(test)]
pub(super) fn current_admission_for_test() -> Option<std::sync::Weak<tokio::sync::Semaphore>> {
    current().map(|lease| Arc::downgrade(lease.semaphore()))
}

#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod tests;
