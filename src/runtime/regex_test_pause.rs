//! Request-scoped synchronization at the regular-expression search boundary.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::CassieError;

type PauseChannels = (SyncSender<bool>, Receiver<()>);

#[derive(Debug)]
pub(super) struct QueryRegexTestPause {
    channels: Mutex<Option<PauseChannels>>,
}

impl QueryRegexTestPause {
    fn new() -> (Self, Receiver<bool>, SyncSender<()>) {
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

    pub(super) fn reach(&self, matched: bool) -> Result<(), CassieError> {
        let channels = self.channels.lock().expect("regex test pause").take();
        if let Some((selected, resume)) = channels {
            selected.send(matched).map_err(|error| {
                CassieError::Execution(format!("regex test observer disconnected: {error}"))
            })?;
            resume
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| {
                    CassieError::Execution(format!("regex test resume failed: {error}"))
                })?;
        }
        Ok(())
    }
}

thread_local! {
    static NEXT_QUERY_PAUSE: RefCell<Option<Arc<super::QueryTestPause>>> =
        const { RefCell::new(None) };
}

/// Holds a pending regex-search pause for the next query handle created on this thread.
///
/// The query handle keeps the pause after it is captured, so dropping this guard cannot
/// affect an active request. If the query never starts, dropping the guard clears it.
#[doc(hidden)]
#[must_use]
pub struct QueryRegexTestPauseGuard {
    pause: Arc<super::QueryTestPause>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for QueryRegexTestPauseGuard {
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

/// Pauses the next query's first `regexp_like`/regex-predicate search after it completes.
/// Other regex scalar functions do not reach this diagnostic boundary.
///
/// Both returned endpoints must be serviced. The selected endpoint reports the completed
/// search's actual match result; resuming lets execution observe cancellation or return its result.
///
/// # Panics
///
/// Panics if another sleep or regex diagnostic pause is pending on this thread.
#[doc(hidden)]
pub fn pause_next_query_after_regex_search(
) -> (QueryRegexTestPauseGuard, Receiver<bool>, SyncSender<()>) {
    assert!(
        !super::sleep_test_pause::has_pending_query_pause(),
        "regex and sleep test pauses must not overlap"
    );
    let (pause, selected, resume) = QueryRegexTestPause::new();
    let pause = Arc::new(super::QueryTestPause::Regex(pause));
    NEXT_QUERY_PAUSE.with(|pending| {
        let mut pending = pending.borrow_mut();
        assert!(pending.is_none(), "regex test pauses must not be nested");
        pending.replace(Arc::clone(&pause));
    });
    (
        QueryRegexTestPauseGuard {
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
    use super::pause_next_query_after_regex_search;
    use crate::app::CassieError;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::time::{Duration, Instant};

    fn controls(cancellation: QueryCancellationHandle) -> QueryExecutionControls {
        QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        )
    }

    #[test]
    fn should_preserve_regex_request_ownership_across_a_single_pause() {
        // Arrange
        let (guard, selected, resume) = pause_next_query_after_regex_search();
        let cancellation = QueryCancellationHandle::new();
        let target = controls(cancellation.clone());
        drop(guard);
        let unrelated = controls(QueryCancellationHandle::new());
        target
            .aggregate_input_retained()
            .expect("different operator");
        target.sleep_function_entered().expect("different operator");
        unrelated
            .regex_search_completed(true)
            .expect("unrelated request");
        assert!(selected.try_recv().is_err());
        let clone = target.for_uncapped_input();
        let worker = std::thread::spawn(move || {
            target
                .regex_search_completed(true)
                .expect("owned regex boundary");
            clone
                .regex_search_completed(true)
                .expect("pause consumed across clones");
            clone.check_interruption()
        });

        // Act
        selected
            .recv_timeout(Duration::from_secs(5))
            .expect("completed regex search");
        cancellation.cancel();
        resume.send(()).expect("resume cancelled regex");
        let result = worker.join().expect("regex worker");

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCancelled)));
        assert!(selected.try_recv().is_err());
    }

    #[test]
    fn should_clear_unclaimed_regex_pause_when_its_guard_drops() {
        // Arrange
        let (guard, selected, _resume) = pause_next_query_after_regex_search();

        // Act
        drop(guard);
        let result = controls(QueryCancellationHandle::new()).regex_search_completed(true);

        // Assert
        assert!(result.is_ok());
        assert!(selected.try_recv().is_err());
    }

    #[test]
    fn should_fail_regex_pause_when_the_observer_disconnects() {
        // Arrange
        let (_guard, selected, _resume) = pause_next_query_after_regex_search();
        let target = controls(QueryCancellationHandle::new());
        drop(selected);

        // Act
        let result = target.regex_search_completed(true);

        // Assert
        assert!(matches!(result, Err(CassieError::Execution(_))));
    }

    #[test]
    fn should_fail_regex_pause_when_the_resumer_disconnects() {
        // Arrange
        let (_guard, selected, resume) = pause_next_query_after_regex_search();
        let target = controls(QueryCancellationHandle::new());
        let worker = std::thread::spawn(move || target.regex_search_completed(true));
        selected
            .recv_timeout(Duration::from_secs(5))
            .expect("target reaches regex boundary");

        // Act
        drop(resume);
        let result = worker.join().expect("regex worker");

        // Assert
        assert!(matches!(result, Err(CassieError::Execution(_))));
    }
}
