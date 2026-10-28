//! Request-scoped synchronization at the SQL sleep-function entry boundary.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::CassieError;

type PauseChannels = (SyncSender<()>, Receiver<()>);

#[derive(Debug)]
pub(super) struct QuerySleepTestPause {
    channels: Mutex<Option<PauseChannels>>,
}

impl QuerySleepTestPause {
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
        let channels = self.channels.lock().expect("sleep test pause").take();
        if let Some((selected, resume)) = channels {
            selected.send(()).map_err(|error| {
                CassieError::Execution(format!("sleep test observer disconnected: {error}"))
            })?;
            resume
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| {
                    CassieError::Execution(format!("sleep test resume failed: {error}"))
                })?;
        }
        Ok(())
    }
}

thread_local! {
    static NEXT_QUERY_PAUSE: RefCell<Option<Arc<super::QueryTestPause>>> =
        const { RefCell::new(None) };
}

/// Holds a pending sleep-entry pause for the next query handle created on this thread.
///
/// The query handle keeps the pause after it is captured, so dropping this guard cannot
/// affect an active request. If the query never starts, dropping the guard clears it.
#[doc(hidden)]
#[must_use]
pub struct QuerySleepTestPauseGuard {
    pause: Arc<super::QueryTestPause>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for QuerySleepTestPauseGuard {
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

/// Pauses the next SQL sleep call after evaluation enters the sleep function.
///
/// Both returned endpoints must be serviced. The selected endpoint reports that the query
/// reached `pg_sleep`; resuming lets execution observe cancellation or continue sleeping.
#[doc(hidden)]
pub fn pause_next_query_at_sleep_entry() -> (QuerySleepTestPauseGuard, Receiver<()>, SyncSender<()>)
{
    assert!(
        !super::aggregate_test_pause::has_pending_query_pause(),
        "sleep and aggregate test pauses must not overlap"
    );
    let (pause, selected, resume) = QuerySleepTestPause::new();
    let pause = Arc::new(super::QueryTestPause::Sleep(pause));
    NEXT_QUERY_PAUSE.with(|pending| {
        let mut pending = pending.borrow_mut();
        assert!(pending.is_none(), "sleep test pauses must not be nested");
        pending.replace(Arc::clone(&pause));
    });
    (
        QuerySleepTestPauseGuard {
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
