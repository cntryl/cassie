//! Writer intent for the catalog publication lock.
//!
//! The publication lock permits recursive reads, so readers never queue behind
//! a waiting writer. Statements hold a read guard for their whole execution;
//! when they overlap continuously, an exclusive capture would never run. A
//! writer therefore announces its intent, and a new top-level statement yields
//! at entry until no writer is waiting. A statement nested on a thread that
//! already entered one never yields, because the writer is waiting for that
//! thread. The yield is bounded, so an entry this bookkeeping cannot see as
//! nested (for example on another thread) delays a writer instead of
//! deadlocking with it.

use std::cell::Cell;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use super::CassieError;
use crate::runtime::QueryExecutionControls;

/// Longest a new statement defers to waiting writers before it proceeds.
const STATEMENT_YIELD_LIMIT: Duration = Duration::from_secs(5);
/// Interval at which a yielding statement re-checks cancellation and deadline.
const STATEMENT_YIELD_SLICE: Duration = Duration::from_millis(5);

thread_local! {
    static STATEMENT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[derive(Default)]
struct IntentState {
    writers: usize,
    yielding: usize,
}

#[derive(Default)]
pub(crate) struct PublicationIntent {
    state: Mutex<IntentState>,
    changed: Condvar,
}

/// Held by a writer from before it first tries the exclusive publication
/// guard until it has finished with it.
pub(crate) struct WriterIntent<'a> {
    intent: &'a PublicationIntent,
}

/// Held by a top-level statement for as long as it holds its publication read
/// guard. It stays on the entering thread.
pub(crate) struct StatementEntry {
    _thread_bound: PhantomData<*const ()>,
}

impl PublicationIntent {
    pub(crate) fn announce(&self) -> WriterIntent<'_> {
        self.state.lock().writers += 1;
        WriterIntent { intent: self }
    }

    /// Enters a statement, first yielding to waiting writers unless this
    /// thread is already inside a statement.
    ///
    /// # Errors
    ///
    /// Returns the cancellation or deadline error observed while yielding.
    pub(crate) fn enter_statement(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<StatementEntry, CassieError> {
        if STATEMENT_DEPTH.with(Cell::get) == 0 {
            self.yield_to_writers(controls)?;
        }
        STATEMENT_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        Ok(StatementEntry {
            _thread_bound: PhantomData,
        })
    }

    fn yield_to_writers(&self, controls: &QueryExecutionControls) -> Result<(), CassieError> {
        let mut state = self.state.lock();
        if state.writers == 0 {
            return Ok(());
        }
        let started = Instant::now();
        state.yielding += 1;
        let mut outcome = Ok(());
        while state.writers > 0 && started.elapsed() < STATEMENT_YIELD_LIMIT {
            self.changed.wait_for(&mut state, STATEMENT_YIELD_SLICE);
            outcome = controls.check_interruption();
            if outcome.is_err() {
                break;
            }
        }
        state.yielding -= 1;
        outcome
    }

    #[cfg(test)]
    pub(crate) fn yielding_statements(&self) -> usize {
        self.state.lock().yielding
    }
}

impl Drop for WriterIntent<'_> {
    fn drop(&mut self) {
        let mut state = self.intent.state.lock();
        state.writers = state.writers.saturating_sub(1);
        drop(state);
        self.intent.changed.notify_all();
    }
}

impl Drop for StatementEntry {
    fn drop(&mut self) {
        STATEMENT_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::QueryCancellationHandle;

    fn canceled_controls() -> QueryExecutionControls {
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        )
    }

    #[test]
    fn should_enter_without_yielding_when_no_writer_is_waiting() {
        // Arrange
        let intent = PublicationIntent::default();

        // Act
        let entry = intent.enter_statement(&canceled_controls());

        // Assert
        assert!(entry.is_ok());
    }

    #[test]
    fn should_observe_cancellation_while_yielding_to_a_writer() {
        // Arrange
        let intent = PublicationIntent::default();
        let _writer = intent.announce();

        // Act
        let entry = intent.enter_statement(&canceled_controls());

        // Assert
        assert!(matches!(entry, Err(CassieError::QueryCanceled)));
        assert_eq!(intent.yielding_statements(), 0);
    }

    #[test]
    fn should_not_yield_for_a_statement_nested_on_the_entering_thread() {
        // Arrange
        let intent = PublicationIntent::default();
        let outer = intent.enter_statement(&canceled_controls());
        let _writer = intent.announce();

        // Act
        let nested = intent.enter_statement(&canceled_controls());

        // Assert
        assert!(outer.is_ok());
        assert!(nested.is_ok());
    }

    #[test]
    fn should_yield_again_after_the_outer_statement_ends() {
        // Arrange
        let intent = PublicationIntent::default();
        let outer = intent.enter_statement(&canceled_controls());
        drop(outer);
        let _writer = intent.announce();

        // Act
        let entry = intent.enter_statement(&canceled_controls());

        // Assert
        assert!(matches!(entry, Err(CassieError::QueryCanceled)));
    }

    #[test]
    fn should_release_yielding_statements_when_the_writer_finishes() {
        // Arrange
        let intent = std::sync::Arc::new(PublicationIntent::default());
        let writer = intent.announce();
        let statement_intent = std::sync::Arc::clone(&intent);
        let statement = std::thread::spawn(move || {
            let controls = QueryExecutionControls::from_limits(
                &CassieRuntimeLimits::default(),
                Instant::now(),
            );
            statement_intent.enter_statement(&controls).is_ok()
        });
        let hang_guard = Instant::now() + Duration::from_secs(4);
        while intent.yielding_statements() == 0 && Instant::now() < hang_guard {
            std::thread::yield_now();
        }
        let yielding = intent.yielding_statements();

        // Act
        drop(writer);
        let entered = statement.join().expect("statement thread");

        // Assert
        assert_eq!(yielding, 1);
        assert!(entered);
    }
}
