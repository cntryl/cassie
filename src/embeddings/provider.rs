use crate::embeddings::{Embedding, EmbeddingError};
use crate::runtime::QueryExecutionControls;
use std::time::{Duration, Instant};

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

pub(crate) fn run_controlled_request<T, F>(
    provider: &str,
    controls: Option<&QueryExecutionControls>,
    request: F,
) -> Result<reqwest::Result<T>, EmbeddingError>
where
    T: Send + 'static,
    F: FnOnce() -> reqwest::Result<T> + Send + 'static,
{
    let Some(controls) = controls else {
        return Ok(request());
    };
    check_controls(provider, controls)?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(request());
    });
    loop {
        match receiver.recv_timeout(Duration::from_millis(5)) {
            Ok(result) => return Ok(result),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                check_controls(provider, controls)?;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
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
