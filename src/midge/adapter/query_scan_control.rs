use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

static TEST_GUARD: OnceLock<parking_lot::RwLock<()>> = OnceLock::new();
static CANCELLATION_AFTER_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static CONTROLLED_ENTRIES: AtomicUsize = AtomicUsize::new(0);

// Fixture setup stays parallel under shared guards. Arming cancellation upgrades only that
// test's controlled section to exclusive access so blocking query workers can use global state.
enum QueryScanControlGuardState {
    Shared {
        guard: parking_lot::RwLockReadGuard<'static, ()>,
    },
    Exclusive {
        guard: parking_lot::RwLockWriteGuard<'static, ()>,
    },
}

thread_local! {
    static TEST_GUARD_STATE: RefCell<Option<QueryScanControlGuardState>> =
        const { RefCell::new(None) };
}

#[doc(hidden)]
#[must_use]
pub struct QueryScanControlTestGuard {
    _guard: PhantomData<parking_lot::RwLockReadGuard<'static, ()>>,
}

impl Drop for QueryScanControlTestGuard {
    fn drop(&mut self) {
        reset_query_scan_control();
        TEST_GUARD_STATE.with(|state| {
            state.borrow_mut().take();
        });
    }
}

#[doc(hidden)]
pub fn query_scan_control_test_guard() -> QueryScanControlTestGuard {
    let guard = TEST_GUARD
        .get_or_init(|| parking_lot::RwLock::new(()))
        .read();
    TEST_GUARD_STATE.with(|state| {
        assert!(
            state.borrow().is_none(),
            "query scan control test guards must not be nested"
        );
        state
            .borrow_mut()
            .replace(QueryScanControlGuardState::Shared { guard });
    });
    reset_query_scan_control();
    QueryScanControlTestGuard {
        _guard: PhantomData,
    }
}

#[doc(hidden)]
pub fn set_query_scan_cancellation_after_entries(entries: Option<usize>) {
    if entries.is_some() {
        transition_test_guard_to_exclusive();
    }
    CONTROLLED_ENTRIES.store(0, Ordering::SeqCst);
    CANCELLATION_AFTER_ENTRIES.store(entries.unwrap_or_default(), Ordering::SeqCst);
    if entries.is_none() {
        transition_test_guard_to_shared();
    }
}

pub(super) fn should_cancel_controlled_query_scan() -> bool {
    let threshold = CANCELLATION_AFTER_ENTRIES.load(Ordering::SeqCst);
    if threshold == 0 {
        return false;
    }
    let entry = CONTROLLED_ENTRIES
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1);
    if entry < threshold {
        return false;
    }
    CANCELLATION_AFTER_ENTRIES.store(0, Ordering::SeqCst);
    true
}

fn reset_query_scan_control() {
    CONTROLLED_ENTRIES.store(0, Ordering::SeqCst);
    CANCELLATION_AFTER_ENTRIES.store(0, Ordering::SeqCst);
}

fn transition_test_guard_to_exclusive() {
    TEST_GUARD_STATE.with(|state| {
        let current = state.borrow_mut().take();
        match current {
            Some(QueryScanControlGuardState::Shared { guard }) => {
                drop(guard);
                let guard = TEST_GUARD
                    .get_or_init(|| parking_lot::RwLock::new(()))
                    .write();
                state
                    .borrow_mut()
                    .replace(QueryScanControlGuardState::Exclusive { guard });
            }
            Some(current @ QueryScanControlGuardState::Exclusive { .. }) => {
                state.borrow_mut().replace(current);
            }
            None => {}
        }
    });
}

fn transition_test_guard_to_shared() {
    TEST_GUARD_STATE.with(|state| {
        let current = state.borrow_mut().take();
        match current {
            Some(QueryScanControlGuardState::Exclusive { guard }) => {
                drop(guard);
                let guard = TEST_GUARD
                    .get_or_init(|| parking_lot::RwLock::new(()))
                    .read();
                state
                    .borrow_mut()
                    .replace(QueryScanControlGuardState::Shared { guard });
            }
            Some(current @ QueryScanControlGuardState::Shared { .. }) => {
                state.borrow_mut().replace(current);
            }
            None => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::{
        query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
        should_cancel_controlled_query_scan,
    };

    #[test]
    fn should_isolate_query_scan_controls_by_test_thread() {
        // Arrange
        let unrelated_guard = query_scan_control_test_guard();
        let armed = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_armed = Arc::clone(&armed);
        let worker_release = Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            let _controlled_guard = query_scan_control_test_guard();
            worker_armed.wait();
            worker_release.wait();
            set_query_scan_cancellation_after_entries(Some(2));
            let controlled = (
                should_cancel_controlled_query_scan(),
                should_cancel_controlled_query_scan(),
            );
            set_query_scan_cancellation_after_entries(None);
            controlled
        });
        armed.wait();

        // Act
        let unrelated = should_cancel_controlled_query_scan();
        drop(unrelated_guard);
        release.wait();
        let controlled = worker.join().expect("query scan control worker");

        // Assert
        assert!(!unrelated);
        assert_eq!(controlled, (false, true));
    }
}
