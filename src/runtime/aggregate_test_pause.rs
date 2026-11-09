//! Request-scoped synchronization for aggregate cancellation tests.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::app::CassieError;

type PauseChannels = (SyncSender<()>, Receiver<()>);

#[derive(Debug)]
pub(super) struct QueryAggregateTestPause {
    channels: Mutex<Option<PauseChannels>>,
}

impl QueryAggregateTestPause {
    pub(super) fn new() -> (Self, Receiver<()>, SyncSender<()>) {
        let (selected_tx, selected_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        (
            Self {
                channels: Mutex::new(Some((selected_tx, resume_rx))),
            },
            selected_rx,
            resume_tx,
        )
    }

    pub(super) fn reach(&self) -> Result<(), CassieError> {
        let channels = self.channels.lock().expect("aggregate test pause").take();
        if let Some((selected, resume)) = channels {
            selected.send(()).map_err(|error| {
                CassieError::Execution(format!("aggregate test observer disconnected: {error}"))
            })?;
            resume
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| {
                    CassieError::Execution(format!("aggregate test resume failed: {error}"))
                })?;
        }
        Ok(())
    }
}

/// Owns a pending pause on one backend until the backend claims it.
#[doc(hidden)]
#[must_use]
pub struct QueryAggregateTestPauseGuard {
    pub(super) runtime: Weak<super::RuntimeState>,
    pub(super) process_id: i32,
    pub(super) pause: Arc<super::QueryTestPause>,
}

impl Drop for QueryAggregateTestPauseGuard {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.clear_backend_aggregate_pause(self.process_id, &self.pause);
        }
    }
}

/// Install a one-shot retained-input pause for a specific Cassie backend.
///
/// This existing diagnostic hook targets the supplied instance and backend
/// identity. A claimed pause follows its query handle and its clones.
///
/// # Errors
///
/// Returns an error if the backend identity is invalid or already has a pause.
#[doc(hidden)]
pub fn pause_next_query_after_aggregate_input(
    cassie: &crate::app::Cassie,
    process_id: i32,
    secret_key: i32,
) -> Result<(QueryAggregateTestPauseGuard, Receiver<()>, SyncSender<()>), CassieError> {
    cassie
        .runtime
        .pause_backend_after_aggregate_input(process_id, secret_key)
}

#[cfg(test)]
pub(crate) fn pause_direct_handle_after_aggregate_input(
    cancellation: &mut super::QueryCancellationHandle,
) -> (Receiver<()>, SyncSender<()>) {
    let (pause, selected, resume) = QueryAggregateTestPause::new();
    assert!(cancellation.attach_test_pause(Arc::new(super::QueryTestPause::Aggregate(pause))));
    (selected, resume)
}

#[cfg(test)]
mod tests {
    use super::pause_direct_handle_after_aggregate_input;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::time::{Duration, Instant};

    #[test]
    fn should_fail_when_aggregate_observer_disconnects() {
        // Arrange
        let mut cancellation = QueryCancellationHandle::new();
        let (selected, _resume) = pause_direct_handle_after_aggregate_input(&mut cancellation);
        drop(selected);
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        );

        // Act
        let result = controls.aggregate_input_retained();

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_fail_when_aggregate_resumer_disconnects() {
        // Arrange
        let mut cancellation = QueryCancellationHandle::new();
        let (selected, resume) = pause_direct_handle_after_aggregate_input(&mut cancellation);
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        );
        let worker = std::thread::spawn(move || controls.aggregate_input_retained());
        selected
            .recv_timeout(Duration::from_secs(1))
            .expect("input reached");

        // Act
        drop(resume);
        let result = worker.join().expect("worker");

        // Assert
        assert!(result.is_err());
    }
}
