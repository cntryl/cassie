//! Discarding a dropped relation's staged row changes.

use super::{Arc, CassieSession, SessionTransactionStatus};

impl CassieSession {
    /// Drops the transaction's staged row changes and conflict intents for a
    /// relation that this transaction just dropped.
    ///
    /// Staged writes are keyed by relation name. Without this, a same-named
    /// relation created later in the transaction would inherit the dropped
    /// relation's rows. Savepoints snapshot the staged writes, so
    /// `ROLLBACK TO` restores them.
    pub(crate) fn discard_staged_writes_for_dropped_relation(&self, collection: &str) {
        let mut transaction = self.transaction.lock();
        if self.ensure_operation_current_locked(&transaction).is_err()
            || transaction.status != SessionTransactionStatus::InTransaction
        {
            return;
        }
        let writes = Arc::make_mut(&mut transaction.writes);
        writes.retain(|name, _| !crate::catalog::name_matches(name, collection));
        transaction
            .conflict_intents
            .retain(|intent| !crate::catalog::name_matches(&intent.schema.collection, collection));
    }
}
