//! Session-local sequence history for `currval` and `lastval`, and the
//! sequence positions a rolled-back `TRUNCATE ... RESTART IDENTITY` restores.
#[cfg(test)]
#[path = "session_sequences/discard_restart_tests.rs"]
mod discard_restart_tests;

use std::collections::HashMap;

use super::{CassieError, CassieSession, SessionTransactionStatus};
use crate::catalog::SequenceError;

#[derive(Default)]
pub(crate) struct SessionSequenceValues {
    by_sequence: HashMap<String, i64>,
    last: Option<(String, u64)>,
}

impl std::fmt::Debug for SessionSequenceValues {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionSequenceValues")
            .field("by_sequence", &self.by_sequence)
            .field("last", &self.last)
            .finish()
    }
}

fn sequence_identity(name: &str, identity: u64) -> String {
    if identity == 0 {
        format!("name:{}", name.to_ascii_lowercase())
    } else {
        format!("id:{identity}")
    }
}

fn not_defined(message: String) -> CassieError {
    CassieError::Sequence(SequenceError {
        sqlstate: "55000",
        message,
    })
}

impl CassieSession {
    /// Records a value returned by `nextval` (or `setval`) for `sequence`.
    pub(crate) fn record_sequence_value(
        &self,
        sequence: &str,
        identity: u64,
        value: i64,
        from_nextval: bool,
    ) {
        // Sequence advances commit independently of the transaction's rows, so
        // they keep the existing strict policy even after a savepoint rollback.
        self.note_projection_effect(true);
        let mut values = self.sequence_values.lock();
        values
            .by_sequence
            .insert(sequence_identity(sequence, identity), value);
        if from_nextval {
            values.last = Some((sequence.to_ascii_lowercase(), identity));
        }
    }

    /// `currval`.
    ///
    /// # Errors
    /// Returns `55000` before this session called `nextval` on `sequence`.
    pub(crate) fn current_sequence_value(
        &self,
        sequence: &str,
        identity: u64,
        display_name: &str,
    ) -> Result<i64, CassieError> {
        self.sequence_values
            .lock()
            .by_sequence
            .get(&sequence_identity(sequence, identity))
            .copied()
            .ok_or_else(|| {
                not_defined(format!(
                    "currval of sequence \"{display_name}\" is not yet defined in this session"
                ))
            })
    }

    /// `DISCARD SEQUENCES`: forgets this session's `currval`/`lastval`
    /// history without changing any sequence.
    pub(crate) fn discard_sequence_values(&self) {
        self.discard_sequence_values_inner(|| {}, |_| {});
    }

    fn discard_sequence_values_inner(
        &self,
        on_contended: impl FnOnce(),
        after_status: impl FnOnce(Option<SessionTransactionStatus>),
    ) {
        #[cfg(not(test))]
        let mut values = {
            let _ = on_contended;
            self.sequence_values.lock()
        };
        #[cfg(test)]
        let mut values = if let Some(values) = self.sequence_values.try_lock() {
            values
        } else {
            on_contended();
            self.sequence_values.lock()
        };
        self.with_root_transaction_status_locked(|status| {
            after_status(status);
            values.by_sequence = HashMap::new();
            values.last = None;
        });
        drop(values);
    }

    #[cfg(test)]
    fn discard_sequence_values_observed(
        &self,
        on_contended: impl FnOnce(),
        after_status: impl FnOnce(Option<SessionTransactionStatus>),
    ) {
        self.discard_sequence_values_inner(on_contended, after_status);
    }

    /// `lastval`.
    ///
    /// # Errors
    /// Returns `55000` before this session called `nextval`.
    pub(crate) fn last_sequence_value(&self) -> Result<i64, CassieError> {
        self.last_sequence_value_inner(|| {})
    }

    #[cfg(test)]
    fn last_sequence_value_observed(&self, after_lock: impl FnOnce()) -> Result<i64, CassieError> {
        self.last_sequence_value_inner(after_lock)
    }

    fn last_sequence_value_inner(&self, after_lock: impl FnOnce()) -> Result<i64, CassieError> {
        let values = self.sequence_values.lock();
        after_lock();
        let (name, identity) = values
            .last
            .as_ref()
            .ok_or_else(|| not_defined("lastval is not yet defined in this session".to_string()))?;
        let (catalog, context) = self
            .object_lookup_context()
            .ok_or_else(|| not_defined("lastval is not yet defined in this session".to_string()))?;
        let exists = if *identity == 0 {
            catalog.sequence_exists(name)
        } else {
            crate::catalog::object_identity::normal_oid(*identity)
                .and_then(|oid| {
                    crate::catalog::object_identity::ObjectScope::new(&catalog, &context)
                        .relation_canonical_name(oid)
                })
                .is_some_and(|canonical| catalog.sequence_exists(&canonical))
        };
        if !exists {
            return Err(not_defined(
                "lastval is not yet defined in this session".to_string(),
            ));
        }
        values
            .by_sequence
            .get(&sequence_identity(name, *identity))
            .copied()
            .ok_or_else(|| not_defined("lastval is not yet defined in this session".to_string()))
    }
}
