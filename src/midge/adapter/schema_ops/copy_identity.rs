//! COPY physical identity from its staged overlay or the current family.

use std::io::{self, Read};

use super::{CassieError, Midge};
use crate::midge::adapter::{StorageOverlay, TransactionMode};
use crate::runtime::QueryExecutionControls;

#[cfg(test)]
#[path = "copy_identity_tests.rs"]
mod tests;

struct CheckingReader<'a> {
    input: &'a [u8],
    position: usize,
    controls: &'a QueryExecutionControls,
}

impl Read for CheckingReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.position % 4096 == 0 {
            self.controls
                .check_interruption()
                .map_err(|_| io::Error::other("query interrupted"))?;
        }
        let count = output
            .len()
            .min(self.input.len() - self.position)
            .min(4096 - self.position % 4096);
        output[..count].copy_from_slice(&self.input[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

#[derive(serde::Deserialize)]
struct Identity {
    #[serde(default)]
    relation_id: u64,
}

pub(super) fn decode_identity(
    raw: &[u8],
    controls: &QueryExecutionControls,
) -> Result<Option<u64>, CassieError> {
    controls.check_interruption()?;
    let _memory = controls.reserve_query_memory(
        crate::runtime::accounted::json::serialized_decode_bytes(raw.len())?,
    )?;
    let reader = CheckingReader {
        input: raw,
        position: 0,
        controls,
    };
    let identity: Identity = serde_json::from_reader(reader).map_err(|error| {
        controls
            .check_interruption()
            .err()
            .unwrap_or_else(|| CassieError::Parse(format!("invalid COPY row descriptor: {error}")))
    })?;
    Ok((identity.relation_id != 0).then_some(identity.relation_id))
}

impl Midge {
    pub(crate) fn copy_physical_identity(
        &self,
        name: &str,
        overlay: &StorageOverlay,
        controls: &QueryExecutionControls,
    ) -> Result<Option<u64>, CassieError> {
        controls.check_interruption()?;
        let canonical = self.canonical_collection_name(name);
        let key = Self::row_schema_key(&canonical);
        if let Some(change) = overlay.changes.get(&key) {
            return change
                .value
                .as_ref()
                .map(|raw| decode_identity(raw, controls))
                .transpose()
                .map(Option::flatten);
        }
        let database = crate::catalog::relation_database_name(&canonical)
            .unwrap_or_else(|| self.default_database.clone());
        let tx = self.begin_raw_database_adapter_tx(&database, TransactionMode::ReadOnly)?;
        let Some(raw) = tx.get(&key).map_err(CassieError::from)? else {
            return Ok(None);
        };
        let _raw_memory = controls.reserve_query_memory(raw.len())?;
        decode_identity(&raw, controls)
    }
}
