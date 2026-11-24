//! Test-only deterministic barrier between materialized joined sources.
use std::cell::RefCell;

thread_local! {
    static AFTER_LEFT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(crate) struct JoinReadProbe;

impl JoinReadProbe {
    pub(crate) fn install(callback: impl FnOnce() + 'static) -> Self {
        AFTER_LEFT.with(|slot| {
            assert!(slot.borrow().is_none(), "one scoped joined-source barrier");
            *slot.borrow_mut() = Some(Box::new(callback));
        });
        Self
    }
}

impl Drop for JoinReadProbe {
    fn drop(&mut self) {
        AFTER_LEFT.with(|slot| slot.borrow_mut().take());
    }
}

pub(super) fn after_left_source_read() {
    let callback = AFTER_LEFT.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}
