//! Session-local sequence history for `currval` and `lastval`.
use std::collections::HashMap;

use super::{CassieError, CassieSession};
use crate::catalog::SequenceError;

#[derive(Debug, Default)]
pub(crate) struct SessionSequenceValues {
    by_sequence: HashMap<String, i64>,
    last: Option<(String, i64)>,
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
}
