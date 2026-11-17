//! Deferred operation authority follows a wire command through worker/output.

use super::{Arc, CassieSession, FailureAuthority, SessionTransactionStatus};

impl FailureAuthority {
    pub(crate) fn capture_for_operation(session: &CassieSession) -> Self {
        Self::capture_for_operation_with_contention(session, || {})
    }

    pub(crate) fn capture_for_operation_with_contention(
        session: &CassieSession,
        on_contention: impl FnOnce(),
    ) -> Self {
        let Some(authority) = session
            .operation_failure_authority
            .as_ref()
            .filter(|authority| authority.matches_root(session))
        else {
            return Self::capture_with_contention(session, on_contention);
        };
        let transaction = if let Some(transaction) = session.transaction.try_lock() {
            transaction
        } else {
            on_contention();
            session.transaction.lock()
        };
        let mut owner = authority.owner.lock();
        if !owner.captured && !owner.admission_rejected {
            owner.identity = match transaction.status {
                SessionTransactionStatus::Idle => None,
                SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed => {
                    Some((transaction.unique_owner_id, transaction.rollback_epoch))
                }
            };
            owner.generation = transaction.generation;
            owner.captured = true;
        }
        drop(owner);
        drop(transaction);
        authority.clone()
    }

    pub(crate) fn rollback_if_current(&self, session: &CassieSession) -> bool {
        if !self.matches_root(session) {
            return false;
        }
        let transaction = session.transaction.lock();
        let mut owner = self.owner.lock();
        let expected = match transaction.status {
            SessionTransactionStatus::Idle => None,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed => {
                Some((transaction.unique_owner_id, transaction.rollback_epoch))
            }
        };
        if !owner.captured
            || owner.admission_rejected
            || owner.identity != expected
            || owner.generation != transaction.generation
            || !transaction.may_be_failed_by_current_thread()
        {
            return false;
        }
        owner.retired_generation = Some(transaction.generation);
        drop(owner);
        session.rollback_transaction_locked(transaction);
        true
    }
}

impl CassieSession {
    pub(crate) fn try_with_current_operation_owner<T>(
        &self,
        publish: impl FnOnce(super::FailureAuthorityIdentity) -> T,
    ) -> Result<T, super::CassieError> {
        let transaction = self
            .transaction
            .try_lock()
            .ok_or_else(super::sequence_effect::changed)?;
        self.ensure_operation_current_locked(&transaction)?;
        if self.operation_failure_authority.is_none() || transaction.sealed_for_commit {
            return Err(super::sequence_effect::changed());
        }
        Ok(publish(super::FailureAuthorityIdentity::from_transaction(
            &transaction,
        )))
    }

    pub(crate) fn try_with_retired_operation_owner(
        &self,
        cleanup: impl FnOnce(super::FailureAuthorityIdentity),
    ) -> bool {
        let Some(authority) = self
            .operation_failure_authority
            .as_ref()
            .filter(|authority| authority.matches_root(self))
        else {
            return false;
        };
        let Some(transaction) = self.transaction.try_lock() else {
            return false;
        };
        let Some(owner) = authority.identity() else {
            return false;
        };
        let current = super::FailureAuthorityIdentity::from_transaction(&transaction);
        let retired = current.transaction.is_none() && current.generation == owner.generation
            || current.transaction != owner.transaction
            || current.generation != owner.generation;
        if retired {
            cleanup(owner);
        }
        retired
    }

    /// Root mutation and ownership validation share this transaction lock.
    pub(super) fn ensure_operation_current_locked(
        &self,
        transaction: &super::SessionTransactionState,
    ) -> Result<(), super::CassieError> {
        self.ensure_given_operation_current_locked(transaction, None)
    }

    pub(super) fn ensure_given_operation_current_locked(
        &self,
        transaction: &super::SessionTransactionState,
        authority: Option<&FailureAuthority>,
    ) -> Result<(), super::CassieError> {
        if !self.owns_transaction_lifecycle {
            return Ok(());
        }
        let Some(authority) = authority.or(self.operation_failure_authority.as_ref()) else {
            return Ok(());
        };
        let owner = authority.owner.lock();
        let expected = match transaction.status {
            SessionTransactionStatus::Idle => None,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed => {
                Some((transaction.unique_owner_id, transaction.rollback_epoch))
            }
        };
        if !authority.matches_root(self)
            || !owner.captured
            || owner.admission_rejected
            || owner.identity != expected
            || owner.generation != transaction.generation
        {
            return Err(super::sequence_effect::changed());
        }
        Ok(())
    }

    pub(super) fn ensure_begin_operation_current_locked(
        &self,
        transaction: &super::SessionTransactionState,
        authority: Option<&FailureAuthority>,
    ) -> Result<(), super::CassieError> {
        let Some(authority) = authority.or(self.operation_failure_authority.as_ref()) else {
            return Ok(());
        };
        let owner = authority.owner.lock();
        let may_start = authority.matches_root(self)
            && !owner.admission_rejected
            && transaction.status == SessionTransactionStatus::Idle
            && (!owner.captured
                || owner.retired_generation == Some(transaction.generation)
                    && owner.generation == transaction.generation);
        drop(owner);
        if may_start {
            Ok(())
        } else {
            self.ensure_given_operation_current_locked(transaction, Some(authority))
        }
    }

    pub(super) fn note_owned_retirement_locked(
        &self,
        transaction: &super::SessionTransactionState,
    ) {
        if let Some(authority) = self
            .operation_failure_authority
            .as_ref()
            .filter(|authority| authority.matches_root(self))
        {
            let mut owner = authority.owner.lock();
            if owner.captured
                && owner.identity == Some((transaction.unique_owner_id, transaction.rollback_epoch))
                && owner.generation == transaction.generation
            {
                owner.retired_generation = Some(transaction.generation);
            }
        }
    }

    pub(crate) fn rollback_for_operation(
        &self,
        authority: Option<&FailureAuthority>,
    ) -> Result<(), super::CassieError> {
        if let Some(authority) = authority.or(self.operation_failure_authority.as_ref()) {
            if !authority.rollback_if_current(self) {
                return Err(super::sequence_effect::changed());
            }
        } else {
            self.rollback_transaction();
        }
        Ok(())
    }

    pub(crate) fn with_new_wire_operation(&self) -> Self {
        let mut operation = self.clone();
        operation.operation_failure_authority = Some(FailureAuthority {
            root: Arc::downgrade(&self.transaction),
            owner: Arc::default(),
        });
        operation
    }

    pub(crate) fn operation_failure_authority(&self) -> Option<&FailureAuthority> {
        self.operation_failure_authority.as_ref()
    }

    pub(crate) fn reject_operation_admission(&self) {
        if let Some(authority) = self.operation_failure_authority.as_ref() {
            authority.owner.lock().admission_rejected = true;
        }
    }

    pub(crate) fn note_operation_worker_started(&self) {
        if let Some(authority) = self.operation_failure_authority.as_ref() {
            authority.owner.lock().worker_started = true;
        }
    }

    pub(crate) fn may_capture_protocol_owner(&self) -> bool {
        self.operation_failure_authority
            .as_ref()
            .is_some_and(|authority| {
                let owner = authority.owner.lock();
                !owner.admission_rejected && (!owner.worker_started || owner.captured)
            })
    }

    pub(crate) fn operation_was_rejected(&self) -> bool {
        self.operation_failure_authority
            .as_ref()
            .is_some_and(|authority| authority.owner.lock().admission_rejected)
    }

    pub(crate) fn fail_operation_transaction(&self) {
        let Some(authority) = self
            .operation_failure_authority
            .as_ref()
            .filter(|authority| authority.matches_root(self))
        else {
            return;
        };
        let mut transaction = self.transaction.lock();
        let owner = authority.owner.lock();
        if !owner.captured
            || owner.admission_rejected
            || owner.identity != Some((transaction.unique_owner_id, transaction.rollback_epoch))
        {
            return;
        }
        drop(owner);
        if transaction.implicit && transaction.may_be_failed_by_current_thread() {
            self.rollback_transaction_locked(transaction);
        } else {
            transaction.fail_or_defer();
        }
    }

    /// Portal cleanup is in-memory and nonblocking; contention retains portals.
    pub(crate) fn try_with_owned_idle_transaction(&self, cleanup: impl FnOnce()) {
        let Some(authority) = self
            .operation_failure_authority
            .as_ref()
            .filter(|authority| authority.matches_root(self))
        else {
            return;
        };
        let Some(transaction) = self.transaction.try_lock() else {
            return;
        };
        let owner = authority.owner.lock();
        if owner.captured
            && !owner.admission_rejected
            && owner.generation == transaction.generation
            && transaction.status == SessionTransactionStatus::Idle
        {
            cleanup();
        }
    }

    pub(crate) fn try_with_owned_transaction(&self, cleanup: impl FnOnce()) {
        let Some(authority) = self
            .operation_failure_authority
            .as_ref()
            .filter(|authority| authority.matches_root(self))
        else {
            return;
        };
        let Some(transaction) = self.transaction.try_lock() else {
            return;
        };
        let owner = authority.owner.lock();
        let matching = owner.identity
            == Some((transaction.unique_owner_id, transaction.rollback_epoch))
            || transaction.status == SessionTransactionStatus::Idle
                && owner.generation == transaction.generation;
        if owner.captured && !owner.admission_rejected && matching && !transaction.sealed_for_commit
        {
            cleanup();
        }
    }
}
