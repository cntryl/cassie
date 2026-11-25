use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::catalog::{name_matches, Catalog};

pub use super::sequence_state::{
    SequenceCursor, SequenceError, SequenceOptions, SequenceParameters,
};
pub use cassie_core::catalog::declarations::SequencePersistence;
pub use cassie_core::catalog::sequence::{SequenceMeta, SequenceOwner};

impl Catalog {
    #[must_use]
    pub fn sequence_store() -> Arc<RwLock<HashMap<String, SequenceMeta>>> {
        Arc::new(RwLock::new(HashMap::new()))
    }

    pub fn register_sequence(&self, metadata: SequenceMeta) {
        self.sequences
            .write()
            .insert(metadata.name.clone(), metadata);
        self.bump_version();
    }

    /// Mirrors nontransactional cursor state for an existing sequence without
    /// changing its structural catalog version.
    pub(crate) fn update_sequence_cursor(&self, metadata: &SequenceMeta) {
        let key = &metadata.name;
        let mut sequences = self.sequences.write();
        // Storage publishes the canonical (database-qualified) name, while the
        // catalog may hold the same sequence under its shorter registered name.
        let target = if sequences.contains_key(key) {
            Some(key.clone())
        } else {
            sequences
                .iter()
                .find(|(_, current)| {
                    current.object_id == metadata.object_id
                        && (name_matches(&current.name, &metadata.name)
                            || name_matches(&metadata.name, &current.name))
                })
                .map(|(stored_key, _)| stored_key.clone())
        };
        if let Some(current) = target.and_then(|key| sequences.get_mut(&key)) {
            if current.object_id == metadata.object_id {
                current.current_value = metadata.current_value;
                current.is_called = metadata.is_called;
            }
        }
    }

    /// Takes the published cursor of every sequence this catalog holds under
    /// the same identity as `published`, except the `staged` identities the
    /// owning transaction advanced or restarted itself. A transaction overlay
    /// would otherwise replay an older cursor over newer published advances.
    pub(crate) fn refresh_published_sequence_cursors(&self, published: &Self, staged: &[u64]) {
        let live = published.sequences.read();
        let mut sequences = self.sequences.write();
        for current in sequences.values_mut() {
            if staged.contains(&current.object_id) {
                continue;
            }
            if let Some(source) = live
                .values()
                .find(|source| source.object_id == current.object_id && source.object_id != 0)
            {
                current.current_value = source.current_value;
                current.is_called = source.is_called;
            }
        }
    }

    pub fn unregister_sequence(&self, name: &str) {
        let mut sequences = self.sequences.write();
        if sequences.remove(name).is_none() {
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
        let sequences = self.sequences.read();
        sequences.get(name).cloned().or_else(|| {
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
        out.sort_by(|left, right| left.name.cmp(&right.name));
        out
    }
}

pub use cassie_sql::sequence_default::{
    canonical_nextval_expression, parse_nextval_default_expression, serial_sequence_name,
};
