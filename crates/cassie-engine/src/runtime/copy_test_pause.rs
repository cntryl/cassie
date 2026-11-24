use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, Weak};
use std::time::Duration;

use crate::app::CassieError;

type PauseChannels = (SyncSender<usize>, Receiver<()>);

#[derive(Debug)]
pub(super) struct CopyTestPause {
    armed: AtomicBool,
    channels: Mutex<Option<PauseChannels>>,
}

impl CopyTestPause {
    pub(super) fn new() -> (Self, Receiver<usize>, SyncSender<()>) {
        let (staged_tx, staged_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        (
            Self {
                armed: AtomicBool::new(true),
                channels: Mutex::new(Some((staged_tx, resume_rx))),
            },
            staged_rx,
            resume_tx,
        )
    }

    pub(super) fn reach(&self, ordinal: usize) -> Result<(), CassieError> {
        if !self.armed.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let channels = self.channels.lock().expect("COPY test pause").take();
        if let Some((staged, resume)) = channels {
            staged.send(ordinal).map_err(|error| {
                CassieError::Execution(format!("COPY test observer disconnected: {error}"))
            })?;
            resume
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| {
                    CassieError::Execution(format!("COPY test resume failed: {error}"))
                })?;
        }
        Ok(())
    }

    fn abort(&self) {
        self.armed.store(false, Ordering::Release);
        self.channels.lock().expect("COPY test pause").take();
    }
}

/// RAII cleanup for a request-owned COPY staging pause.
#[doc(hidden)]
#[must_use]
pub struct CopyTestPauseGuard {
    pause: Weak<CopyTestPause>,
    staged: Receiver<usize>,
    resume: SyncSender<()>,
}

impl CopyTestPauseGuard {
    pub(super) fn new(
        pause: Weak<CopyTestPause>,
        staged: Receiver<usize>,
        resume: SyncSender<()>,
    ) -> Self {
        Self {
            pause,
            staged,
            resume,
        }
    }

    /// Wait for the request to stage the first row.
    ///
    /// # Errors
    ///
    /// Returns the channel timeout or disconnect error when no row reaches the boundary.
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<usize, std::sync::mpsc::RecvTimeoutError> {
        self.staged.recv_timeout(timeout)
    }

    /// Resume the paused request.
    ///
    /// # Errors
    ///
    /// Returns the channel send error when the request is no longer waiting.
    pub fn send(&self) -> Result<(), std::sync::mpsc::SendError<()>> {
        self.resume.send(())
    }
}

impl Drop for CopyTestPauseGuard {
    fn drop(&mut self) {
        if let Some(pause) = self.pause.upgrade() {
            // Release an active request before aborting it, so a guard dropped
            // under an assertion or early return cannot leave COPY blocked.
            let _ = self.resume.try_send(());
            pause.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CopyTestPause;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn should_consume_copy_pause_once_across_control_clones() {
        // Arrange
        let mut owned = QueryCancellationHandle::new();
        let (pause, staged, resume) = CopyTestPause::new();
        let pause = Arc::new(pause);
        owned.attach_copy_test_pause(Arc::clone(&pause));
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            owned,
        );
        let clone = controls.for_uncapped_input();
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let first = controls.copy_row_staged(1);
            let second = clone.copy_row_staged(2);
            result_tx
                .send((first, second))
                .expect("send COPY pause result");
        });

        // Act
        let reached = staged.recv_timeout(Duration::from_secs(10));
        let _ = resume.send(());
        let results = result_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("COPY pause worker result");
        worker.join().expect("COPY pause worker");

        // Assert
        assert_eq!(reached.expect("first COPY staging boundary"), 1);
        assert!(results.0.is_ok());
        assert!(results.1.is_ok());
        assert!(staged.try_recv().is_err());
    }

    #[test]
    fn should_restrict_copy_pause_to_its_request() {
        // Arrange
        let mut owned = QueryCancellationHandle::new();
        let (pause, staged, resume) = CopyTestPause::new();
        owned.attach_copy_test_pause(Arc::new(pause));
        let owned_controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            owned,
        );
        let unrelated =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

        // Act
        let unrelated_result = unrelated.copy_row_staged(1);
        let not_reached = staged.try_recv();
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            result_tx
                .send(owned_controls.copy_row_staged(1))
                .expect("send owned COPY pause result");
        });
        let reached = staged.recv_timeout(Duration::from_secs(10));
        let _ = resume.send(());
        let owned_result = result_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("owned COPY pause worker result");
        worker.join().expect("owned COPY pause worker");

        // Assert
        assert!(unrelated_result.is_ok());
        assert!(matches!(
            not_reached,
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert_eq!(reached.expect("owned COPY staging boundary"), 1);
        assert!(owned_result.is_ok());
    }

    #[test]
    fn should_drop_copy_pause_guard_with_worker_released() {
        // Arrange
        let mut owned = QueryCancellationHandle::new();
        let pause_guard = owned.pause_after_copy_row_staged();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            owned,
        );
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            result_tx
                .send(controls.copy_row_staged(1))
                .expect("send COPY drop-release result");
        });
        let staged_row_ordinal = pause_guard
            .recv_timeout(Duration::from_secs(10))
            .expect("COPY staging boundary");
        assert_eq!(staged_row_ordinal, 1);

        // Act
        drop(pause_guard);

        // Assert
        let result = result_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("COPY worker released by dropped guard");
        assert!(result.is_ok());
        worker.join().expect("COPY drop-release worker");
    }
}
