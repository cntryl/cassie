//! Request-owned diagnostics for synchronizing cancellation after retained scoring work.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Mutex;
use std::time::Duration;

use crate::app::CassieError;

/// An operator boundary supported by the hidden cancellation test diagnostic.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryScoringTestBoundary {
    ExactVector,
    PersistedFulltext,
}

type PauseChannels = (SyncSender<()>, Receiver<()>);

#[derive(Debug)]
pub(super) struct ScoringTestPause {
    boundary: QueryScoringTestBoundary,
    channels: Mutex<Option<PauseChannels>>,
}

impl ScoringTestPause {
    pub(super) fn new(boundary: QueryScoringTestBoundary) -> (Self, Receiver<()>, SyncSender<()>) {
        let (selected_tx, selected_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        (
            Self {
                boundary,
                channels: Mutex::new(Some((selected_tx, resume_rx))),
            },
            selected_rx,
            resume_tx,
        )
    }

    pub(super) fn reach(&self, boundary: QueryScoringTestBoundary) -> Result<(), CassieError> {
        if self.boundary != boundary {
            return Ok(());
        }
        // Consume the hook before waiting, and never hold its lock across synchronization.
        let channels = self.channels.lock().expect("scoring test pause").take();
        if let Some((selected, resume)) = channels {
            selected.send(()).map_err(|error| {
                CassieError::Execution(format!("scoring test observer disconnected: {error}"))
            })?;
            resume
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| {
                    CassieError::Execution(format!("scoring test resume failed: {error}"))
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::QueryScoringTestBoundary::{ExactVector, PersistedFulltext};
    use crate::app::CassieError;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::time::{Duration, Instant};

    #[test]
    fn should_leave_interruption_to_the_operator_when_no_pause_is_installed() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();
        // Act
        let hook = controls.scored_candidate(ExactVector);
        let interruption = controls.check_interruption();
        // Assert
        assert!(
            hook.is_ok(),
            "the diagnostic hook must not mask the operator check"
        );
        assert!(matches!(interruption, Err(CassieError::QueryCancelled)));
    }

    #[test]
    fn should_restrict_scoring_pause_to_its_request_operator_boundary() {
        // Arrange
        let mut owned = QueryCancellationHandle::new();
        let (selected, resume) = owned.pause_after_scored_candidate(ExactVector);
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            owned.clone(),
        );
        let unrelated =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

        // Act
        let wrong_operator = controls.scored_candidate(PersistedFulltext);
        let other_request = unrelated.scored_candidate(ExactVector);
        let not_reached = selected.try_recv();
        let worker = std::thread::spawn(move || {
            let hook = controls.scored_candidate(ExactVector);
            (hook, controls.check_interruption())
        });
        let reached = selected.recv_timeout(Duration::from_secs(10));
        owned.cancel();
        let _ = resume.send(());
        let result = worker.join().expect("scoring worker");

        // Assert
        assert!(wrong_operator.is_ok());
        assert!(other_request.is_ok());
        assert!(matches!(
            not_reached,
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        reached.expect("owned scoring boundary");
        assert!(result.0.is_ok());
        assert!(matches!(result.1, Err(CassieError::QueryCancelled)));
    }

    #[test]
    fn should_consume_scoring_pause_once_across_control_clones() {
        // Arrange
        let mut owned = QueryCancellationHandle::new();
        let (selected, resume) = owned.pause_after_scored_candidate(PersistedFulltext);
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            owned,
        );
        let clone = controls.for_uncapped_input();
        let worker = std::thread::spawn(move || {
            let first = controls.scored_candidate(PersistedFulltext);
            let second = clone.scored_candidate(PersistedFulltext);
            (first, second)
        });

        // Act
        let reached = selected.recv_timeout(Duration::from_secs(10));
        let _ = resume.send(());
        let results = worker.join().expect("scoring worker");

        // Assert
        reached.expect("first scoring boundary");
        assert!(results.0.is_ok());
        assert!(results.1.is_ok());
        assert!(selected.try_recv().is_err());
    }
}
