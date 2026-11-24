//! Per-operation authority for converted-query failure completion.

use super::{Arc, CassieSession, Mutex, SessionTransactionState, SessionTransactionStatus};
use std::sync::Weak;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct FailureAuthorityIdentity {
    pub transaction: Option<Uuid>,
    pub generation: u64,
    pub rollback_epoch: u64,
}

impl FailureAuthorityIdentity {
    pub(super) fn from_transaction(transaction: &SessionTransactionState) -> Self {
        Self {
            transaction: (transaction.status != SessionTransactionStatus::Idle)
                .then_some(transaction.unique_owner_id),
            generation: transaction.generation,
            rollback_epoch: if transaction.status == SessionTransactionStatus::Idle {
                0
            } else {
                transaction.rollback_epoch
            },
        }
    }
}

/// Owned by one operation and explicitly borrowed by its nested work.
#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct FailureAuthority {
    pub(super) root: Weak<Mutex<SessionTransactionState>>,
    pub(super) owner: Arc<Mutex<OperationOwner>>,
}

#[derive(Debug, Default)]
pub(super) struct OperationOwner {
    pub(super) identity: Option<(Uuid, u64)>,
    pub(super) generation: u64,
    pub(super) captured: bool,
    pub(super) admission_rejected: bool,
    pub(super) worker_started: bool,
    pub(super) expected_portal: Option<FailureAuthorityIdentity>,
    portal_adoption: Option<(FailureAuthorityIdentity, FailureAuthorityIdentity)>,
    pub(super) retired_generation: Option<u64>,
    shared_memory: Option<crate::runtime::QueryMemoryReservation>,
}

impl FailureAuthority {
    #[doc(hidden)]
    pub fn identity(&self) -> Option<FailureAuthorityIdentity> {
        let owner = self.owner.lock();
        (owner.captured && !owner.admission_rejected).then_some(FailureAuthorityIdentity {
            transaction: owner.identity.map(|(identity, _)| identity),
            generation: owner.generation,
            rollback_epoch: owner.identity.map_or(0, |(_, epoch)| epoch),
        })
    }

    pub(crate) fn portal_adoption(
        &self,
    ) -> Option<(FailureAuthorityIdentity, FailureAuthorityIdentity)> {
        let owner = self.owner.lock();
        if owner.captured && !owner.admission_rejected {
            owner.portal_adoption
        } else {
            None
        }
    }

    #[doc(hidden)]
    pub fn reserve_shared_owner(
        &self,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), super::CassieError> {
        let mut owner = self.owner.lock();
        if owner.shared_memory.is_none() {
            owner.shared_memory = Some(controls.reserve_query_memory(Self::shared_owner_bytes())?);
        }
        Ok(())
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    pub(crate) fn shared_owner_reservation_matches_for_test(
        &self,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> bool {
        self.owner
            .lock()
            .shared_memory
            .as_ref()
            .is_some_and(|memory| memory.belongs_to(controls))
    }

    /// Retained shared owner payload and Arc counters, excluding allocator metadata.
    #[doc(hidden)]
    pub const fn shared_owner_bytes() -> usize {
        std::mem::size_of::<Mutex<OperationOwner>>() + 2 * std::mem::size_of::<usize>()
    }

    #[cfg(test)]
    pub(crate) fn identity_only_for_test(session: &CassieSession) -> Self {
        Self {
            root: Arc::downgrade(&session.transaction),
            owner: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn owner_for_test(&self) -> Option<(Uuid, u64)> {
        self.owner.lock().identity
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
        Self::captured_from_state(session, &transaction)
    }

    pub(super) fn captured_from_state(
        session: &CassieSession,
        transaction: &SessionTransactionState,
    ) -> Self {
        let identity = match transaction.status {
            SessionTransactionStatus::Idle => None,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed => {
                Some((transaction.unique_owner_id, transaction.rollback_epoch))
            }
        };
        Self {
            root: Arc::downgrade(&session.transaction),
            owner: Arc::new(Mutex::new(OperationOwner {
                identity,
                generation: transaction.generation,
                captured: true,
                admission_rejected: false,
                worker_started: false,
                expected_portal: None,
                portal_adoption: None,
                retired_generation: None,
                shared_memory: None,
            })),
        }
    }

    pub(super) fn matches_root(&self, session: &CassieSession) -> bool {
        std::ptr::eq(self.root.as_ptr(), Arc::as_ptr(&session.transaction))
    }

    /// Caller holds the root transaction mutex after a successful new start.
    pub(super) fn adopt_start(
        &self,
        session: &CassieSession,
        transaction: &SessionTransactionState,
        before: FailureAuthorityIdentity,
    ) {
        if self.matches_root(session) {
            let mut owner = self.owner.lock();
            owner.portal_adoption = before.transaction.is_none().then_some((
                before,
                FailureAuthorityIdentity::from_transaction(transaction),
            ));
            owner.identity = Some((transaction.unique_owner_id, transaction.rollback_epoch));
            owner.generation = transaction.generation;
            owner.captured = true;
            owner.retired_generation = None;
            owner.expected_portal = None;
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
        if owner.captured && owner.identity == Some((transaction.unique_owner_id, previous)) {
            owner.identity = Some((transaction.unique_owner_id, transaction.rollback_epoch));
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
            self.owner.lock().identity = Some((transaction.unique_owner_id, previous));
        }
    }

    pub(crate) fn mark_failed_if_current(&self, session: &CassieSession) {
        let mut transaction = session.transaction.lock();
        let owner = self.owner.lock();
        if self.matches_root(session)
            && owner.captured
            && !owner.admission_rejected
            && owner.identity == Some((transaction.unique_owner_id, transaction.rollback_epoch))
        {
            transaction.fail_or_defer();
        }
    }

    pub(super) fn admit_sequence_effect(
        &self,
        session: &CassieSession,
    ) -> Result<(), super::CassieError> {
        if !session.owns_transaction_lifecycle && self.matches_root(session) {
            return Err(super::sequence_effect::changed());
        }
        let root = self
            .root
            .upgrade()
            .ok_or_else(super::sequence_effect::changed)?;
        let transaction = root.lock();
        let expected = match transaction.status {
            SessionTransactionStatus::Idle => None,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed => {
                Some((transaction.unique_owner_id, transaction.rollback_epoch))
            }
        };
        let owner = self.owner.lock();
        if !owner.captured
            || owner.admission_rejected
            || owner.identity != expected
            || owner.generation != transaction.generation
        {
            return Err(super::sequence_effect::changed());
        }
        drop(owner);
        if transaction.status == SessionTransactionStatus::Failed {
            return Err(super::CassieError::failed_transaction());
        }
        let projection_matches = if self.matches_root(session) {
            true
        } else {
            Arc::ptr_eq(
                &transaction.projection_write,
                &session.transaction.lock().projection_write,
            )
        };
        if !projection_matches
            || transaction.sealed_for_commit
                && (!session.owns_transaction_lifecycle
                    || !transaction.may_be_failed_by_current_thread())
        {
            return Err(super::sequence_effect::changed());
        }
        transaction.projection_write.note_effect(true);
        Ok(())
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
