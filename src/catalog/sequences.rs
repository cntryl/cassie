use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::catalog::{derive_scoped_name, name_matches};
use crate::types::DataType;

pub use super::sequence_state::{
    SequenceCursor, SequenceError, SequenceOptions, SequenceParameters,
};

use super::Catalog;

/// The column that owns a sequence (`OWNED BY table.column`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SequenceOwner {
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SequencePersistence {
    #[default]
    Permanent,
    Unlogged,
    Temporary,
}

impl SequencePersistence {
    /// `pg_class.relpersistence`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Permanent => "p",
            Self::Unlogged => "u",
            Self::Temporary => "t",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SequenceMeta {
    pub name: String,
    pub data_type: DataType,
    pub start_value: i64,
    pub increment_by: i64,
    /// PostgreSQL's `last_value`.
    pub current_value: i64,
    /// Whether `current_value` has been returned by `nextval`.
    pub is_called: bool,
    pub min_value: i64,
    pub max_value: i64,
    pub cache: i64,
    pub cycle: bool,
    #[serde(default)]
    pub owned_by: Option<SequenceOwner>,
    #[serde(default)]
    pub persistence: SequencePersistence,
    /// Stable relation identity allocated at creation; renames keep it.
    #[serde(default)]
    pub object_id: u64,
}

impl SequenceMeta {
    /// A sequence with PostgreSQL's default options for `data_type`.
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        let parameters = SequenceParameters::resolve(None, &SequenceOptions::default(), &data_type)
            .unwrap_or(SequenceParameters {
                data_type,
                start: 1,
                increment: 1,
                min_value: 1,
                max_value: i64::MAX,
                cache: 1,
                cycle: false,
            });
        Self::with_parameters(name, parameters)
    }

    /// A sequence with resolved `parameters`, positioned at its start.
    pub fn with_parameters(name: impl Into<String>, parameters: SequenceParameters) -> Self {
        Self {
            name: name.into(),
            current_value: parameters.start,
            is_called: false,
            data_type: parameters.data_type,
            start_value: parameters.start,
            increment_by: parameters.increment,
            min_value: parameters.min_value,
            max_value: parameters.max_value,
            cache: parameters.cache,
            cycle: parameters.cycle,
            owned_by: None,
            persistence: SequencePersistence::Permanent,
            object_id: 0,
        }
    }

    #[must_use]
    pub fn parameters(&self) -> SequenceParameters {
        SequenceParameters {
            data_type: self.data_type.clone(),
            start: self.start_value,
            increment: self.increment_by,
            min_value: self.min_value,
            max_value: self.max_value,
            cache: self.cache,
            cycle: self.cycle,
        }
    }

    #[must_use]
    pub const fn cursor(&self) -> SequenceCursor {
        SequenceCursor {
            last_value: self.current_value,
            is_called: self.is_called,
        }
    }

    pub fn set_parameters(&mut self, parameters: SequenceParameters) {
        self.data_type = parameters.data_type;
        self.start_value = parameters.start;
        self.increment_by = parameters.increment;
        self.min_value = parameters.min_value;
        self.max_value = parameters.max_value;
        self.cache = parameters.cache;
        self.cycle = parameters.cycle;
    }

    pub const fn set_cursor(&mut self, cursor: SequenceCursor) {
        self.current_value = cursor.last_value;
        self.is_called = cursor.is_called;
    }

    /// The unqualified relation name.
    #[must_use]
    pub fn local_name(&self) -> String {
        crate::catalog::local_name(&self.name)
    }

    /// `nextval`, applied in place.
    ///
    /// # Errors
    /// Returns `2200H` when a non-cycling sequence is exhausted.
    pub fn advance(&mut self) -> Result<i64, SequenceError> {
        let mut cursor = self.cursor();
        let value = cursor.next(&self.local_name(), &self.parameters())?;
        self.set_cursor(cursor);
        Ok(value)
    }

    /// `setval`, applied in place.
    ///
    /// # Errors
    /// Returns `22003` when `value` is outside the sequence bounds.
    pub fn assign(&mut self, value: i64, is_called: bool) -> Result<i64, SequenceError> {
        let mut cursor = self.cursor();
        let value = cursor.set(&self.local_name(), &self.parameters(), value, is_called)?;
        self.set_cursor(cursor);
        Ok(value)
    }
}

impl Catalog {
    #[must_use]
    pub fn sequence_store() -> Arc<RwLock<HashMap<String, SequenceMeta>>> {
        Arc::new(RwLock::new(HashMap::new()))
    }

    pub fn register_sequence(&self, metadata: SequenceMeta) {
        self.sequences
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_sequence(&self, name: &str) {
        let mut sequences = self.sequences.write();
        let key = name.to_ascii_lowercase();
        if sequences.remove(&key).is_none() {
            let matching_key = sequences
                .iter()
                .find(|(_, sequence)| name_matches(&sequence.name, name))
                .map(|(stored_key, _)| stored_key.clone());
            if let Some(stored_key) = matching_key {
                sequences.remove(&stored_key);
            }
        }
        self.bump_version();
    }

    #[must_use]
    pub fn get_sequence(&self, name: &str) -> Option<SequenceMeta> {
        let key = name.to_ascii_lowercase();
        let sequences = self.sequences.read();
        sequences.get(&key).cloned().or_else(|| {
            sequences
                .values()
                .find(|sequence| name_matches(&sequence.name, name))
                .cloned()
        })
    }

    #[must_use]
    pub fn sequence_exists(&self, name: &str) -> bool {
        self.get_sequence(name).is_some()
    }

    #[must_use]
    pub fn list_sequences(&self) -> Vec<SequenceMeta> {
        let mut out = self.sequences.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|sequence| sequence.name.to_ascii_lowercase());
        out
    }
}

#[must_use]
pub fn serial_sequence_name(table: &str, field: &str) -> String {
    derive_scoped_name(table, |local| format!("{local}_{field}_seq"))
}

#[must_use]
pub fn canonical_nextval_expression(sequence: &str) -> String {
    format!("nextval('{sequence}'::regclass)")
}

#[must_use]
pub fn parse_nextval_default_expression(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let lower = raw.to_ascii_lowercase();
    let inner = lower.strip_prefix("nextval(")?;
    if !inner.ends_with(')') {
        return None;
    }

    let original_inner = &raw["nextval(".len()..raw.len().saturating_sub(1)];
    let mut value = original_inner.trim();
    if let Some((candidate, cast)) = value.split_once("::") {
        if !cast.trim().eq_ignore_ascii_case("regclass") {
            return None;
        }
        value = candidate.trim();
    }

    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Some(value[1..value.len() - 1].replace("''", "'"));
    }
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        return Some(value[1..value.len() - 1].replace("\"\"", "\""));
    }
    if is_identifier(value) {
        return Some(value.to_string());
    }
    None
}

fn is_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}
