use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

static TEST_GUARD: OnceLock<parking_lot::Mutex<()>> = OnceLock::new();
static CANCELLATION_AFTER_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static CONTROLLED_ENTRIES: AtomicUsize = AtomicUsize::new(0);

#[doc(hidden)]
#[must_use]
pub struct QueryScanControlTestGuard {
    _guard: parking_lot::MutexGuard<'static, ()>,
}

impl Drop for QueryScanControlTestGuard {
    fn drop(&mut self) {
        set_query_scan_cancellation_after_entries(None);
    }
}

#[doc(hidden)]
pub fn query_scan_control_test_guard() -> QueryScanControlTestGuard {
    let guard = TEST_GUARD
        .get_or_init(|| parking_lot::Mutex::new(()))
        .lock();
    set_query_scan_cancellation_after_entries(None);
    QueryScanControlTestGuard { _guard: guard }
}

#[doc(hidden)]
pub fn set_query_scan_cancellation_after_entries(entries: Option<usize>) {
    CONTROLLED_ENTRIES.store(0, Ordering::SeqCst);
    CANCELLATION_AFTER_ENTRIES.store(entries.unwrap_or_default(), Ordering::SeqCst);
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
