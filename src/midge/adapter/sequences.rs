use super::{Midge, CassieError, key_encoding, WriteOptions, StorageFamily};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_sequence(&self, metadata: crate::catalog::SequenceMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key_encoding::sequence_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_sequence(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::SequenceMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&key_encoding::sequence_key(name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid sequence metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_sequences(&self) -> Result<Vec<crate::catalog::SequenceMeta>, CassieError> {
        let entries =
            self.raw_scan_prefix(StorageFamily::Schema, &key_encoding::sequence_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|sequence: &crate::catalog::SequenceMeta| {
            sequence.name.to_ascii_lowercase()
        });
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_sequence(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(key_encoding::sequence_key(name))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn next_sequence_value(&self, name: &str) -> Result<i64, CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let raw = tx
            .get(&key_encoding::sequence_key(name))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::NotFound(format!("sequence '{name}' does not exist")))?;
        let mut metadata: crate::catalog::SequenceMeta =
            serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid sequence metadata for '{name}': {error}"))
            })?;
        let next = metadata
            .current_value
            .checked_add(metadata.increment_by)
            .ok_or_else(|| CassieError::Execution(format!("sequence '{name}' overflow")))?;
        metadata.current_value = next;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key_encoding::sequence_key(name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(next)
    }
}
