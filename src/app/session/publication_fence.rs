//! Per-session command-publication fence for staged storage and catalog state.
//!
//! A statement that stages storage and then separately publishes catalog state
//! (transactional DDL) must not be observed half-applied by another statement
//! on the same session. Lock order is always the global catalog publication
//! guard, then this fence, then the session transaction mutex. The fence is
//! re-entrant per thread so nested execution on one session cannot deadlock.

use super::{CassieError, CassieSession};
use crate::runtime::QueryExecutionControls;
use parking_lot::{ArcReentrantMutexGuard, RawMutex, RawThreadId, ReentrantMutex};
use std::sync::Arc;
use std::time::Duration;

pub(super) type PublicationFence = Arc<ReentrantMutex<()>>;

/// Holds the session fence for one statement on the current thread.
///
/// Readers release it after capturing their statement view; statements that
/// mutate catalog state keep it through execution. Dropping always releases it.
pub(crate) struct PublicationFenceScope {
    guard: Option<ArcReentrantMutexGuard<RawMutex, RawThreadId, ()>>,
    hold_through_execution: bool,
}

impl PublicationFenceScope {
    /// A scope that holds nothing, for statements that never take the fence.
    pub(crate) const fn none() -> Self {
        Self { guard: None, hold_through_execution: false }
    }

    /// Readers drop the fence once their statement view is captured.
    /// Statements that stage and publish catalog state keep it until dropped.
    pub(crate) fn release_after_capture(&mut self) {
        if !self.hold_through_execution {
            self.guard.take();
        }
    }
}

impl CassieSession {
    /// Acquires the fence, honoring cancellation and deadlines while waiting.
    pub(crate) fn enter_publication_fence(
        &self,
        controls: &QueryExecutionControls,
        hold_through_execution: bool,
    ) -> Result<PublicationFenceScope, CassieError> {
        let mut waited = false;
        loop {
            controls.check_interruption()?;
            if let Some(guard) =
                self.publication_fence.try_lock_arc_for(Duration::from_millis(2))
            {
                return Ok(PublicationFenceScope { guard: Some(guard), hold_through_execution });
            }
            if !waited {
                waited = true;
                #[cfg(test)]
                crate::app::statement_read_tests::checkpoint_capture_waiting();
            }
        }
    }
}
