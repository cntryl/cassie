//! Synchronous cursor-boundary witnesses; reset even when a test unwinds.
use crate::runtime::QueryCancellationHandle;
use std::cell::{Cell, RefCell};

thread_local! {
    static COLLECTION_COUNT: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
    static CANCEL: RefCell<Option<(usize, QueryCancellationHandle)>> = const { RefCell::new(None) };
}

pub(in crate::executor::execution) fn after_row() {
    COUNT.with(|count| {
        count.set(count.get() + 1);
        CANCEL.with(|cancel| {
            if let Some((after, handle)) = cancel.borrow().as_ref() {
                if count.get() == *after {
                    handle.cancel();
                }
            }
        });
    });
}

pub(in crate::executor::execution) fn after_collection_row() {
    COLLECTION_COUNT.with(|count| count.set(count.get() + 1));
}

pub(in crate::executor::execution) fn run<T>(
    cancel: Option<(usize, QueryCancellationHandle)>,
    run: impl FnOnce() -> T,
) -> (T, usize) {
    let (result, generated, _) = run_all(cancel, run);
    (result, generated)
}

pub(in crate::executor::execution) fn run_all<T>(
    cancel: Option<(usize, QueryCancellationHandle)>,
    run: impl FnOnce() -> T,
) -> (T, usize, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            COUNT.with(|count| count.set(0));
            COLLECTION_COUNT.with(|count| count.set(0));
            CANCEL.with(|cancel| *cancel.borrow_mut() = None);
        }
    }
    COUNT.with(|count| count.set(0));
    CANCEL.with(|slot| *slot.borrow_mut() = cancel);
    COLLECTION_COUNT.with(|count| count.set(0));
    let _reset = Reset;
    let result = run();
    (
        result,
        COUNT.with(Cell::get),
        COLLECTION_COUNT.with(Cell::get),
    )
}
