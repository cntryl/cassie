//! Session-level `SET`/`RESET` results made inside a transaction.
//!
//! `SET LOCAL` restoration at COMMIT must not undo a later or earlier
//! session-level write of the same setting, so the transaction remembers the
//! last such write per setting (`None` means the setting is back to its
//! default) and savepoints restore that record on `ROLLBACK TO`.

use std::collections::BTreeMap;

use super::{CassieError, CassieSession, SessionTransactionState, SessionTransactionStatus};

pub(super) type SessionWrites = BTreeMap<String, Option<String>>;

impl CassieSession {
    /// Runs a session-level setting write and, inside a transaction, records
    /// the stored result of every setting in `names`.
    pub(super) fn apply_session_write<T>(
        &self,
        names: &[&str],
        write: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.with_owned_setting_transaction(|transaction| {
            let value = write()?;
            // Statement forks keep private settings, so the record stays with
            // the fork's own transaction, as for `SET LOCAL`.
            if self.owns_transaction_lifecycle {
                self.record_session_write_locked(transaction, names);
            } else if transaction.status == SessionTransactionStatus::InTransaction {
                self.record_session_write_locked(&mut self.transaction.lock(), names);
            }
            Ok(value)
        })
    }

    fn record_session_write_locked(
        &self,
        transaction: &mut SessionTransactionState,
        names: &[&str],
    ) {
        if transaction.status != SessionTransactionStatus::InTransaction {
            return;
        }
        let settings = self.settings.lock();
        for name in names {
            if let Some((canonical, stored)) = settings.stored_entry(name) {
                transaction
                    .session_writes
                    .insert(canonical.to_owned(), stored);
            }
        }
    }
}
