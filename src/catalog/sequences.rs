use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::types::DataType;

use super::Catalog;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SequenceMeta {
    pub name: String,
    pub data_type: DataType,
    pub start_value: i64,
    pub increment_by: i64,
    pub current_value: i64,
}

impl SequenceMeta {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            start_value: 1,
            increment_by: 1,
            current_value: 0,
        }
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
        self.sequences.write().remove(&name.to_ascii_lowercase());
        self.bump_version();
    }

    #[must_use]
    pub fn get_sequence(&self, name: &str) -> Option<SequenceMeta> {
        self.sequences
            .read()
            .get(&name.to_ascii_lowercase())
            .cloned()
    }

    #[must_use]
    pub fn sequence_exists(&self, name: &str) -> bool {
        self.sequences
            .read()
            .contains_key(&name.to_ascii_lowercase())
    }

    #[must_use]
    pub fn list_sequences(&self) -> Vec<SequenceMeta> {
        let mut out = self.sequences.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|sequence| sequence.name.to_ascii_lowercase());
        out
    }

    pub fn set_sequence_current_value(&self, name: &str, current_value: i64) {
        if let Some(sequence) = self.sequences.write().get_mut(&name.to_ascii_lowercase()) {
            sequence.current_value = current_value;
            self.bump_version();
        }
    }
}

#[must_use]
pub fn serial_sequence_name(table: &str, field: &str) -> String {
    format!("{table}_{field}_seq")
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
