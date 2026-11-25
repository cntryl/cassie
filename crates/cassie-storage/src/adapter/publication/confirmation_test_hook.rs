type ConfirmationCallback = Box<dyn FnOnce()>;

thread_local! {
    static NEXT_CONFIRMATION: std::cell::RefCell<Option<ConfirmationCallback>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn observe_confirmation() {
    let callback = NEXT_CONFIRMATION.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

pub(super) fn next_confirmation(callback: impl FnOnce() + 'static) -> impl Drop {
    struct Restore(Option<ConfirmationCallback>);
    impl Drop for Restore {
        fn drop(&mut self) {
            drop(NEXT_CONFIRMATION.with(|slot| slot.replace(self.0.take())));
        }
    }
    Restore(NEXT_CONFIRMATION.with(|slot| slot.replace(Some(Box::new(callback)))))
}
