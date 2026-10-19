//! Session-local sequence history for `currval` and `lastval`, and the
//! sequence positions a rolled-back `TRUNCATE ... RESTART IDENTITY` restores.
use std::collections::HashMap;
use std::sync::Arc;

use super::{CassieError, CassieSession};
use crate::catalog::{SequenceCursor, SequenceError};

/// Writes a sequence position back when its transaction rolls back.
pub(crate) type SequenceRestorer = Arc<dyn Fn(&str, SequenceCursor) + Send + Sync>;

#[derive(Default)]
pub(crate) struct SessionSequenceValues {
    by_sequence: HashMap<String, i64>,
    last: Option<(String, i64)>,
    restarts: Vec<(String, SequenceCursor, SequenceRestorer)>,
}

impl std::fmt::Debug for SessionSequenceValues {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionSequenceValues")
            .field("by_sequence", &self.by_sequence)
            .field("last", &self.last)
            .field("restarts", &self.restarts.len())
            .finish()
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
    pub(crate) fn record_sequence_value(&self, sequence: &str, value: i64, from_nextval: bool) {
        let mut values = self.sequence_values.lock();
        values
            .by_sequence
            .insert(sequence.to_ascii_lowercase(), value);
        if from_nextval {
            values.last = Some((sequence.to_ascii_lowercase(), value));
        }
    }

    /// `currval`.
    ///
    /// # Errors
    /// Returns `55000` before this session called `nextval` on `sequence`.
    pub(crate) fn current_sequence_value(
        &self,
        sequence: &str,
        display_name: &str,
    ) -> Result<i64, CassieError> {
        self.sequence_values
            .lock()
            .by_sequence
            .get(&sequence.to_ascii_lowercase())
            .copied()
            .ok_or_else(|| {
                not_defined(format!(
                    "currval of sequence \"{display_name}\" is not yet defined in this session"
                ))
            })
    }

    /// `lastval`.
    ///
    /// # Errors
    /// Returns `55000` before this session called `nextval`.
    pub(crate) fn last_sequence_value(&self) -> Result<i64, CassieError> {
        self.sequence_values
            .lock()
            .last
            .as_ref()
            .map(|(_, value)| *value)
            .ok_or_else(|| not_defined("lastval is not yet defined in this session".to_string()))
    }

    /// Remembers `cursor`, the position `sequence` had before the current
    /// transaction restarted it, unless an earlier restart already did.
    pub(crate) fn record_sequence_restart(
        &self,
        sequence: &str,
        cursor: SequenceCursor,
        restorer: &SequenceRestorer,
    ) {
        let mut values = self.sequence_values.lock();
        if values
            .restarts
            .iter()
            .any(|(name, _, _)| name.eq_ignore_ascii_case(sequence))
        {
            return;
        }
        values
            .restarts
            .push((sequence.to_string(), cursor, Arc::clone(restorer)));
    }

    /// Forgets restart positions once their transaction commits.
    pub(crate) fn discard_sequence_restarts(&self) {
        self.sequence_values.lock().restarts.clear();
    }

    /// Restores each restarted sequence when its transaction rolls back.
    pub(crate) fn restore_sequence_restarts(&self) {
        let restarts = std::mem::take(&mut self.sequence_values.lock().restarts);
        for (sequence, cursor, restore) in restarts.into_iter().rev() {
            restore(&sequence, cursor);
        }
    }
}
