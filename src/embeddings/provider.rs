use crate::embeddings::{Embedding, EmbeddingError};
use crate::runtime::QueryExecutionControls;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static ACTIVE_CONTROLLED_REQUEST_WORKERS: AtomicUsize = AtomicUsize::new(0);

struct ControlledRequestWorkerGuard;

impl ControlledRequestWorkerGuard {
    fn new() -> Self {
        ACTIVE_CONTROLLED_REQUEST_WORKERS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for ControlledRequestWorkerGuard {
    fn drop(&mut self) {
        ACTIVE_CONTROLLED_REQUEST_WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[doc(hidden)]
#[must_use]
pub fn active_controlled_request_workers_for_diagnostics() -> usize {
    ACTIVE_CONTROLLED_REQUEST_WORKERS.load(Ordering::Acquire)
}

pub trait EmbeddingProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn model_name(&self) -> &str;
    fn dimensions(&self) -> usize;
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError>;
    /// # Errors
    ///
    /// Returns an error when the query is cancelled, its deadline expires, or the provider fails.
    fn embed_documents_with_controls(
        &self,
        inputs: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        check_controls(self.provider_name(), controls)?;
        let embeddings = self.embed_documents(inputs)?;
        check_controls(self.provider_name(), controls)?;
        Ok(embeddings)
    }
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    fn embed_query(&self, input: &str) -> Result<Embedding, EmbeddingError> {
        self.embed_documents(std::slice::from_ref(&input.to_string()))
            .map(|batch| {
                batch
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| Embedding { values: Vec::new() })
            })
    }
    /// # Errors
    ///
    /// Returns an error when the query is cancelled, its deadline expires, or the provider fails.
    fn embed_query_with_controls(
        &self,
        input: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Embedding, EmbeddingError> {
        self.embed_documents_with_controls(std::slice::from_ref(&input.to_string()), controls)
            .map(|batch| {
                batch
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| Embedding { values: Vec::new() })
            })
    }
}

pub(crate) fn check_controls(
    provider: &str,
    controls: &QueryExecutionControls,
) -> Result<(), EmbeddingError> {
    if controls.is_cancelled() {
        return Err(EmbeddingError::Cancelled {
            provider: provider.to_string(),
        });
    }
    if controls.is_timed_out() {
        return Err(EmbeddingError::Timeout {
            provider: provider.to_string(),
            message: "query deadline exceeded".to_string(),
        });
    }
    Ok(())
}

pub(crate) fn controlled_request_timeout(
    provider: &str,
    configured: Duration,
    controls: Option<&QueryExecutionControls>,
) -> Result<Duration, EmbeddingError> {
    let Some(controls) = controls else {
        return Ok(configured);
    };
    check_controls(provider, controls)?;
    let Some(deadline) = controls.deadline else {
        return Ok(configured);
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        check_controls(provider, controls)?;
    }
    Ok(configured.min(remaining))
}

pub(crate) fn controlled_backoff(
    provider: &str,
    delay: Duration,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), EmbeddingError> {
    let Some(controls) = controls else {
        std::thread::sleep(delay);
        return Ok(());
    };
    let started = Instant::now();
    while started.elapsed() < delay {
        check_controls(provider, controls)?;
        let remaining = delay.saturating_sub(started.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(5)));
    }
    check_controls(provider, controls)
}

enum ControlledRequestOutcome<T, E> {
    Completed(Result<T, E>),
    Cancelled,
    RuntimeError(String),
}

fn join_controlled_request_worker(
    worker: &mut Option<std::thread::JoinHandle<()>>,
    provider: &str,
) -> Result<(), EmbeddingError> {
    let Some(worker) = worker.take() else {
        return Err(EmbeddingError::RequestError(format!(
            "{provider} request worker was already joined"
        )));
    };
    worker
        .join()
        .map_err(|_| EmbeddingError::RequestError(format!("{provider} request worker panicked")))
}

pub(crate) fn run_controlled_request<T, E, F>(
    provider: &str,
    controls: Option<&QueryExecutionControls>,
    request: F,
) -> Result<Result<T, E>, EmbeddingError>
where
    T: Send + 'static,
    E: Send + 'static,
    F: Future<Output = Result<T, E>> + Send + 'static,
{
    if let Some(controls) = controls {
        check_controls(provider, controls)?;
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let (cancel_sender, cancel_receiver) = tokio::sync::oneshot::channel();
    let worker = std::thread::Builder::new()
        .name(format!("cassie-{provider}-request"))
        .spawn(move || {
            let _worker_guard = ControlledRequestWorkerGuard::new();
            let outcome = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async move {
                    tokio::select! {
                        result = request => ControlledRequestOutcome::Completed(result),
                        _ = cancel_receiver => ControlledRequestOutcome::Cancelled,
                    }
                }),
                Err(error) => ControlledRequestOutcome::RuntimeError(error.to_string()),
            };
            let _ = sender.send(outcome);
        })
        .map_err(|error| {
            EmbeddingError::RequestError(format!(
                "{provider} request worker could not start: {error}"
            ))
        })?;
    let mut worker = Some(worker);
    let mut cancel_sender = Some(cancel_sender);
    loop {
        match receiver.recv_timeout(Duration::from_millis(5)) {
            Ok(ControlledRequestOutcome::Completed(result)) => {
                join_controlled_request_worker(&mut worker, provider)?;
                return Ok(result);
            }
            Ok(ControlledRequestOutcome::Cancelled) => {
                join_controlled_request_worker(&mut worker, provider)?;
                return Err(EmbeddingError::Cancelled {
                    provider: provider.to_string(),
                });
            }
            Ok(ControlledRequestOutcome::RuntimeError(message)) => {
                join_controlled_request_worker(&mut worker, provider)?;
                return Err(EmbeddingError::RequestError(format!(
                    "{provider} request runtime failed: {message}"
                )));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(controls) = controls {
                    if let Err(control_error) = check_controls(provider, controls) {
                        if let Some(cancel_sender) = cancel_sender.take() {
                            let _ = cancel_sender.send(());
                        }
                        join_controlled_request_worker(&mut worker, provider)?;
                        return Err(control_error);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                join_controlled_request_worker(&mut worker, provider)?;
                return Err(EmbeddingError::RequestError(format!(
                    "{provider} request worker stopped without a response"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    struct RecordingProvider(AtomicBool);

    impl EmbeddingProvider for RecordingProvider {
        fn provider_name(&self) -> &'static str {
            "recording"
        }

        fn model_name(&self) -> &'static str {
            "test"
        }

        fn dimensions(&self) -> usize {
            1
        }

        fn embed_documents(&self, _inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
            self.0.store(true, Ordering::Release);
            Ok(vec![Embedding { values: vec![1.0] }])
        }
    }

    #[test]
    fn should_reject_cancelled_controlled_embedding_before_provider_call() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        );
        let provider = RecordingProvider(AtomicBool::new(false));

        // Act
        let result = provider.embed_query_with_controls("hello", &controls);

        // Assert
        assert!(matches!(result, Err(EmbeddingError::Cancelled { .. })));
        assert!(!provider.0.load(Ordering::Acquire));
    }
}
