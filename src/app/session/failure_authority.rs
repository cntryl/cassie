//! Per-operation authority for converted-query failure completion.

use super::{Arc, CassieSession, Mutex, SessionTransactionState, SessionTransactionStatus};
use std::sync::Weak;
use uuid::Uuid;

/// Owned by one operation and explicitly borrowed by its nested work.
pub(crate) struct FailureAuthority {
    root: Weak<Mutex<SessionTransactionState>>,
    owner: Mutex<Option<(Uuid, u64)>>,
}

impl FailureAuthority {
    #[cfg(test)]
    pub(crate) fn identity_only_for_test(session: &CassieSession) -> Self {
        Self {
            root: Arc::downgrade(&session.transaction),
            owner: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn owner_for_test(&self) -> Option<(Uuid, u64)> {
        *self.owner.lock()
    }

    #[cfg(test)]
    pub(crate) fn captures_session_for_test(&self, session: &CassieSession) -> bool {
        self.matches_root(session)
    }

    pub(crate) fn capture(session: &CassieSession) -> Self {
        Self::capture_with_contention(session, || {})
    }

    pub(crate) fn capture_with_contention(
        session: &CassieSession,
        on_contention: impl FnOnce(),
    ) -> Self {
        let transaction = if let Some(transaction) = session.transaction.try_lock() {
            transaction
        } else {
            on_contention();
            session.transaction.lock()
        };
        let owner = match transaction.status {
            SessionTransactionStatus::Idle => None,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed => {
                Some((transaction.unique_owner_id, transaction.rollback_epoch))
            }
        };
        Self {
            root: Arc::downgrade(&session.transaction),
            owner: Mutex::new(owner),
        }
    }

    fn matches_root(&self, session: &CassieSession) -> bool {
        std::ptr::eq(self.root.as_ptr(), Arc::as_ptr(&session.transaction))
    }

    /// Caller holds the root transaction mutex after a successful new start.
    pub(super) fn adopt_start(
        &self,
        session: &CassieSession,
        transaction: &SessionTransactionState,
    ) {
        if self.matches_root(session) {
            *self.owner.lock() = Some((transaction.unique_owner_id, transaction.rollback_epoch));
        }
    }

    /// Caller holds the root transaction mutex before savepoint pruning.
    pub(super) fn advance_savepoint(
        &self,
        session: &CassieSession,
        transaction: &SessionTransactionState,
        previous: u64,
    ) -> bool {
        if !self.matches_root(session) {
            return false;
        }
        let mut owner = self.owner.lock();
        if *owner == Some((transaction.unique_owner_id, previous)) {
            *owner = Some((transaction.unique_owner_id, transaction.rollback_epoch));
            true
        } else {
            false
        }
    }

    /// Caller holds the root transaction mutex after an ordinary prune error.
    pub(super) fn revert_savepoint(
        &self,
        session: &CassieSession,
        transaction: &SessionTransactionState,
        previous: u64,
    ) {
        if self.matches_root(session) {
            *self.owner.lock() = Some((transaction.unique_owner_id, previous));
        }
    }

    pub(crate) fn mark_failed_if_current(&self, session: &CassieSession) {
        let mut transaction = session.transaction.lock();
        if self.matches_root(session)
            && *self.owner.lock() == Some((transaction.unique_owner_id, transaction.rollback_epoch))
        {
            transaction.fail_or_defer();
        }
    }
}

#[cfg(test)]
impl CassieSession {
    pub(crate) fn hold_transaction_for_test(
        &self,
        entered: &std::sync::mpsc::SyncSender<()>,
        release: &std::sync::mpsc::Receiver<()>,
        held: Arc<std::sync::atomic::AtomicBool>,
    ) {
        struct Held(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Held {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        let _transaction = self.transaction.lock();
        held.store(true, std::sync::atomic::Ordering::Release);
        let _held = Held(held);
        entered.send(()).expect("transaction holder entered");
        let _ = release.recv();
    }
}
