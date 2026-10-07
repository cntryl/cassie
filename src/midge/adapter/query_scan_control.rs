use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::runtime::QueryExecutionControls;

#[derive(Debug, Default)]
pub(crate) struct QueryScanControlScope {
    cancellation_after_entries: AtomicUsize,
    controlled_entries: AtomicUsize,
}

impl QueryScanControlScope {
    fn reset(&self, entries: Option<usize>) {
        self.controlled_entries.store(0, Ordering::SeqCst);
        self.cancellation_after_entries
            .store(entries.unwrap_or_default(), Ordering::SeqCst);
    }

    fn should_cancel(&self) -> bool {
        let threshold = self.cancellation_after_entries.load(Ordering::SeqCst);
        if threshold == 0 {
            return false;
        }
        let entry = self
            .controlled_entries
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        if entry < threshold {
            return false;
        }
        self.cancellation_after_entries.store(0, Ordering::SeqCst);
        true
    }
}

// A fixture owns one request scope. Handles created in that fixture retain the
// scope across worker dispatch; unrelated and expired fixture scopes cannot consume it.
thread_local! {
    static TEST_GUARD_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static TEST_SCOPE: RefCell<Option<Arc<QueryScanControlScope>>> =
        const { RefCell::new(None) };
}

#[doc(hidden)]
#[must_use]
pub struct QueryScanControlTestGuard {
    scope: Arc<QueryScanControlScope>,
    _guard: PhantomData<parking_lot::RwLockReadGuard<'static, ()>>,
}

impl Drop for QueryScanControlTestGuard {
    fn drop(&mut self) {
        self.scope.reset(None);
        TEST_SCOPE.with(|scope| {
            scope.borrow_mut().take();
        });
        TEST_GUARD_ACTIVE.with(|active| active.set(false));
    }
}

#[doc(hidden)]
pub fn query_scan_control_test_guard() -> QueryScanControlTestGuard {
    TEST_GUARD_ACTIVE.with(|active| {
        assert!(
            !active.replace(true),
            "query scan control test guards must not be nested"
        );
    });
    let scope = Arc::new(QueryScanControlScope::default());
    TEST_SCOPE.with(|current| {
        current.borrow_mut().replace(Arc::clone(&scope));
    });
    QueryScanControlTestGuard {
        scope,
        _guard: PhantomData,
    }
}

/// Arms the current fixture scope, rather than a process-wide cancellation hook.
/// Acquire the test guard before constructing cancellation handles or controls.
/// Without a guard, construct handles after arming; disarming discards that scope.
#[doc(hidden)]
pub fn set_query_scan_cancellation_after_entries(entries: Option<usize>) {
    TEST_SCOPE.with(|current| {
        let mut current = current.borrow_mut();
        if entries.is_some() {
            current.get_or_insert_with(|| Arc::new(QueryScanControlScope::default()));
        }
        if let Some(scope) = current.as_ref() {
            scope.reset(entries);
        }
        if entries.is_none() && !TEST_GUARD_ACTIVE.with(Cell::get) {
            current.take();
        }
    });
}

pub(crate) fn current_query_scan_control_scope() -> Option<Arc<QueryScanControlScope>> {
    TEST_SCOPE.with(|scope| scope.borrow().clone())
}

pub(super) fn should_cancel_controlled_query_scan(controls: &QueryExecutionControls) -> bool {
    controls
        .query_scan_control_scope()
        .is_some_and(QueryScanControlScope::should_cancel)
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
            let worker_controls = controls();
            worker_armed.wait();
            worker_release.wait();
            set_query_scan_cancellation_after_entries(Some(2));
            let controlled = (
                should_cancel_controlled_query_scan(&worker_controls),
                should_cancel_controlled_query_scan(&worker_controls),
            );
            set_query_scan_cancellation_after_entries(None);
            controlled
        });
        armed.wait();

        // Act
        let unrelated = should_cancel_controlled_query_scan(&controls());
        drop(unrelated_guard);
        release.wait();
        let controlled = worker.join().expect("query scan control worker");

        // Assert
        assert!(!unrelated);
        assert_eq!(controlled, (false, true));
    }

    fn controls() -> crate::runtime::QueryExecutionControls {
        crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        )
    }

    fn hook(controls: &crate::runtime::QueryExecutionControls) -> bool {
        should_cancel_controlled_query_scan(controls)
    }

    #[test]
    fn should_preserve_the_owned_scan_cancellation_boundary() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let owned = controls();
        let worker_controls = owned.clone();
        set_query_scan_cancellation_after_entries(Some(2));
        let ready = Arc::new(Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let unrelated = std::thread::spawn(move || {
            let unrelated = controls();
            worker_ready.wait();
            hook(&unrelated)
        });
        ready.wait();

        // Act
        let unrelated = unrelated.join().expect("unrelated scanner");
        let owned = std::thread::spawn(move || (hook(&worker_controls), hook(&worker_controls)))
            .join()
            .expect("owned blocking scanner");
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(!unrelated);
        assert_eq!(owned, (false, true));
    }

    #[test]
    fn should_keep_late_workers_outside_a_new_fixture_scope() {
        // Arrange
        let first_guard = query_scan_control_test_guard();
        let old_controls = controls();
        drop(first_guard);
        let _new_guard = query_scan_control_test_guard();
        let new_controls = controls();
        set_query_scan_cancellation_after_entries(Some(2));

        // Act
        let old = std::thread::spawn(move || (hook(&old_controls), hook(&old_controls)))
            .join()
            .expect("late old scanner");
        let new = (hook(&new_controls), hook(&new_controls));
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert_eq!(old, (false, false));
        assert_eq!(new, (false, true));
    }

    #[test]
    fn should_preserve_scope_through_cancellation_control_variants() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let cancellation = crate::runtime::QueryCancellationHandle::default();
        let controlled = crate::runtime::QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        let cloned = controlled.clone();
        let uncapped = controlled.for_uncapped_input();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let before = hook(&controlled);
        let worker = std::thread::spawn(move || (hook(&cloned), hook(&uncapped)))
            .join()
            .expect("controlled worker variants");
        set_query_scan_cancellation_after_entries(None);
        let disarmed = hook(&controlled);
        set_query_scan_cancellation_after_entries(Some(2));
        let rearmed = (hook(&controlled), hook(&controlled));
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(!before);
        assert_eq!(worker, (false, true));
        assert!(!disarmed);
        assert_eq!(rearmed, (false, true));
        assert!(!cancellation.is_cancelled());
        assert!(cancellation.is_same_request(&cancellation.clone()));
    }

    #[test]
    fn should_preserve_outer_scope_after_nested_guard_rejection() {
        // Arrange
        let guard = query_scan_control_test_guard();
        let owned = controls();

        // Act
        let nested = std::panic::catch_unwind(query_scan_control_test_guard);
        set_query_scan_cancellation_after_entries(Some(2));
        let boundary = (hook(&owned), hook(&owned));
        set_query_scan_cancellation_after_entries(None);
        drop(guard);
        let _next = query_scan_control_test_guard();
        let next = controls();
        set_query_scan_cancellation_after_entries(Some(1));
        let next_boundary = hook(&next);
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(nested.is_err());
        assert_eq!(boundary, (false, true));
        assert!(next_boundary);
    }

    #[test]
    fn should_discard_unguarded_scope_after_disarm() {
        // Arrange
        set_query_scan_cancellation_after_entries(Some(2));
        let old = controls();
        set_query_scan_cancellation_after_entries(None);
        set_query_scan_cancellation_after_entries(Some(2));
        let new = controls();

        // Act
        let old = std::thread::spawn(move || (hook(&old), hook(&old)))
            .join()
            .expect("late unguarded scanner");
        let new = (hook(&new), hook(&new));
        set_query_scan_cancellation_after_entries(None);
        let cleared = super::current_query_scan_control_scope().is_none();

        // Assert
        assert_eq!(old, (false, false));
        assert_eq!(new, (false, true));
        assert!(cleared);
    }
}
