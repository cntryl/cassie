//! Request-scoped synchronization for aggregate cancellation tests.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::CassieError;

type PauseChannels = (SyncSender<()>, Receiver<()>);

#[derive(Debug)]
pub(super) struct QueryAggregateTestPause {
    channels: Mutex<Option<PauseChannels>>,
}

impl QueryAggregateTestPause {
    fn new() -> (Self, Receiver<()>, SyncSender<()>) {
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

thread_local! {
    static NEXT_QUERY_PAUSE: RefCell<Option<Arc<super::QueryTestPause>>> =
        const { RefCell::new(None) };
}

/// Holds a pending aggregate pause for the next query handle created on this thread.
///
/// The query handle keeps the pause after it is captured, so dropping this guard
/// cannot affect an active request. If the query never starts, dropping the guard
/// clears the pending pause.
#[doc(hidden)]
#[must_use]
pub struct QueryAggregateTestPauseGuard {
    pause: Arc<super::QueryTestPause>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for QueryAggregateTestPauseGuard {
    fn drop(&mut self) {
        NEXT_QUERY_PAUSE.with(|pending| {
            let mut pending = pending.borrow_mut();
            if pending
                .as_ref()
                .is_some_and(|pause| Arc::ptr_eq(pause, &self.pause))
            {
                pending.take();
            }
        });
    }
}

/// Installs a one-shot pause after aggregate state and its memory charge are retained.
///
/// The next `QueryCancellationHandle` created on the calling thread captures this
/// pause. Both returned endpoints must be serviced; disconnecting either endpoint
/// makes the diagnostic query fail instead of hanging indefinitely.
#[doc(hidden)]
pub fn pause_next_query_after_aggregate_input(
) -> (QueryAggregateTestPauseGuard, Receiver<()>, SyncSender<()>) {
    assert!(
        !super::sleep_test_pause::has_pending_query_pause(),
        "aggregate and sleep test pauses must not overlap"
    );
    let (pause, selected, resume) = QueryAggregateTestPause::new();
    let pause = Arc::new(super::QueryTestPause::Aggregate(pause));
    NEXT_QUERY_PAUSE.with(|pending| {
        let mut pending = pending.borrow_mut();
        assert!(
            pending.is_none(),
            "aggregate test pauses must not be nested"
        );
        pending.replace(Arc::clone(&pause));
    });
    (
        QueryAggregateTestPauseGuard {
            pause,
            _thread_bound: PhantomData,
        },
        selected,
        resume,
    )
}

pub(super) fn take_next_query_pause() -> Option<Arc<super::QueryTestPause>> {
    NEXT_QUERY_PAUSE.with(|pending| pending.borrow_mut().take())
}

pub(super) fn has_pending_query_pause() -> bool {
    NEXT_QUERY_PAUSE.with(|pending| pending.borrow().is_some())
}

#[cfg(test)]
mod tests {
    use super::pause_next_query_after_aggregate_input;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::thread;
    use std::time::{Duration, Instant};

    fn controls(cancellation: QueryCancellationHandle) -> QueryExecutionControls {
        QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        )
    }

    #[test]
    fn should_pause_only_the_next_query_after_aggregate_input_is_retained() {
        // Arrange
        let (guard, selected, resume) = pause_next_query_after_aggregate_input();
        let target = controls(QueryCancellationHandle::new());
        drop(guard);
        let unrelated = controls(QueryCancellationHandle::new());
        let query = thread::spawn(move || target.aggregate_input_retained());

        // Act
        unrelated
            .aggregate_input_retained()
            .expect("unrelated query has no pause");
        selected
            .recv_timeout(Duration::from_secs(1))
            .expect("target query reaches retained aggregate input");
        resume.send(()).expect("resume target query");
        let result = query.join().expect("target query thread");

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn should_fail_aggregate_pause_when_observer_disconnects() {
        // Arrange
        let (_guard, selected, _resume) = pause_next_query_after_aggregate_input();
        let controls = controls(QueryCancellationHandle::new());
        drop(selected);

        // Act
        let result = controls.aggregate_input_retained();

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_clear_pending_aggregate_pause_when_guard_drops_before_query_creation() {
        // Arrange
        let (guard, selected, _resume) = pause_next_query_after_aggregate_input();

        // Act
        drop(guard);
        let controls = controls(QueryCancellationHandle::new());
        let result = controls.aggregate_input_retained();
        let selected_state = selected.try_recv();

        // Assert
        assert!(result.is_ok());
        assert!(selected_state.is_err());
    }

    #[test]
    fn should_fail_aggregate_pause_when_resumer_disconnects() {
        // Arrange
        let (_guard, selected, resume) = pause_next_query_after_aggregate_input();
        let controls = controls(QueryCancellationHandle::new());
        let query = thread::spawn(move || controls.aggregate_input_retained());
        selected
            .recv_timeout(Duration::from_secs(1))
            .expect("target query reaches retained aggregate input");
        drop(resume);

        // Act
        let result = query.join().expect("target query thread");

        // Assert
        assert!(result.is_err());
    }
}
