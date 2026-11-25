//! Admit independent sequence effects under their operation's root owner.

use super::{CassieError, CassieSession, FailureAuthority};

pub(super) fn changed() -> CassieError {
    CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::InvalidState,
        message: "session transaction changed before the operation could take effect".into(),
    }
}

impl CassieSession {
    pub(crate) fn with_operation_failure_authority(
        &self,
        authority: Option<&FailureAuthority>,
    ) -> Self {
        let mut session = self.clone();
        if let Some(authority) = authority {
            session.operation_failure_authority = Some(authority.clone());
        }
        session
    }

    pub(crate) fn admit_sequence_projection_effect(&self) -> Result<(), CassieError> {
        if let Some(authority) = self.operation_failure_authority.as_ref() {
            authority.admit_sequence_effect(self)?;
        } else {
            if !self.owns_transaction_lifecycle {
                return Err(changed());
            }
            // Direct embedded default APIs do not have a query lifecycle. Their
            // admission point is this call, rather than an earlier SQL owner.
            FailureAuthority::capture(self).admit_sequence_effect(self)?;
        }
        #[cfg(all(test, feature = "replay-benchmark-observation"))]
        test_hooks::after_admission();
        Ok(())
    }
}

#[cfg(all(test, feature = "replay-benchmark-observation"))]
pub(super) mod test_hooks {
    use std::cell::RefCell;
    use std::sync::mpsc::{Receiver, SyncSender};
    use std::time::Duration;

    thread_local! {
        static NEXT: RefCell<Option<(SyncSender<()>, Receiver<()>)>> = const { RefCell::new(None) };
    }

    pub(crate) struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            NEXT.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    pub(crate) fn pause_after_admission(entered: SyncSender<()>, resume: Receiver<()>) -> Guard {
        NEXT.with(|slot| assert!(slot.replace(Some((entered, resume))).is_none()));
        Guard
    }

    pub(super) fn after_admission() {
        if let Some((entered, resume)) = NEXT.with(|slot| slot.borrow_mut().take()) {
            entered.send(()).expect("sequence admitted");
            resume
                .recv_timeout(Duration::from_secs(5))
                .expect("resume sequence effect");
        }
    }
}
