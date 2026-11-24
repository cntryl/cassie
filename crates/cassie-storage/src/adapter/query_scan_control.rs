use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct QueryScanControlScope {
    cancellation_after_entries: AtomicUsize,
    controlled_entries: AtomicUsize,
}

impl QueryScanControlScope {
    fn reset(&self, entries: Option<usize>) {
        self.controlled_entries.store(0, Ordering::SeqCst);
        self.cancellation_after_entries
            .store(entries.unwrap_or_default(), Ordering::SeqCst);
    }

    pub(super) fn should_cancel(&self) -> bool {
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

pub fn current_query_scan_control_scope() -> Option<Arc<QueryScanControlScope>> {
    TEST_SCOPE.with(|scope| scope.borrow().clone())
}

pub fn should_cancel_controlled_query_scan(
    context: &super::StorageRequestContext<'_>,
) -> bool {
    context.should_cancel_controlled_query_scan()
}
