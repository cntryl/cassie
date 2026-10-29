use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::runtime::QueryCancellationHandle;

thread_local! {
    static CANCEL_BEFORE_ACTIVATION_PUBLICATION:
        RefCell<Option<QueryCancellationHandle>> = const {
            RefCell::new(None)
        };
    static CANCEL_AFTER_ACTIVATION_INTERRUPTION_CHECK:
        RefCell<Option<QueryCancellationHandle>> = const {
            RefCell::new(None)
        };
}

pub(in crate::executor::execution) struct ActivationPublicationCancellationGuard {
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for ActivationPublicationCancellationGuard {
    fn drop(&mut self) {
        CANCEL_BEFORE_ACTIVATION_PUBLICATION.with(|pending| {
            pending.borrow_mut().take();
        });
        CANCEL_AFTER_ACTIVATION_INTERRUPTION_CHECK.with(|pending| {
            pending.borrow_mut().take();
        });
    }
}

pub(in crate::executor::execution) fn cancel_before_activation_publication(
    cancellation: QueryCancellationHandle,
) -> ActivationPublicationCancellationGuard {
    CANCEL_BEFORE_ACTIVATION_PUBLICATION.with(|pending| {
        assert!(
            pending.borrow_mut().replace(cancellation).is_none(),
            "activation-publication cancellation hooks must not be nested"
        );
    });
    ActivationPublicationCancellationGuard {
        _thread_bound: PhantomData,
    }
}

pub(in crate::executor::execution) fn cancel_after_activation_interruption_check(
    cancellation: QueryCancellationHandle,
) -> ActivationPublicationCancellationGuard {
    CANCEL_AFTER_ACTIVATION_INTERRUPTION_CHECK.with(|pending| {
        assert!(
            pending.borrow_mut().replace(cancellation).is_none(),
            "post-check activation cancellation hooks must not be nested"
        );
    });
    ActivationPublicationCancellationGuard {
        _thread_bound: PhantomData,
    }
}

pub(super) fn cancel_activation_before_publication_if_armed() {
    CANCEL_BEFORE_ACTIVATION_PUBLICATION.with(|pending| {
        if let Some(cancellation) = pending.borrow_mut().take() {
            cancellation.cancel();
        }
    });
}

pub(super) fn cancel_activation_after_interruption_check_if_armed() {
    CANCEL_AFTER_ACTIVATION_INTERRUPTION_CHECK.with(|pending| {
        if let Some(cancellation) = pending.borrow_mut().take() {
            cancellation.cancel();
        }
    });
}
