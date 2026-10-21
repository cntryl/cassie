use super::{key_encoding, CassieError, Midge};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_sequence(&self, metadata: &crate::catalog::SequenceMeta) -> Result<(), CassieError> {
        let mut metadata = metadata.clone();
        metadata.name = self.canonical_collection_name(&metadata.name);
        let mut tx = self.begin_data_rw_tx_for(&metadata.name)?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key_encoding::sequence_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_sequence(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::SequenceMeta>, CassieError> {
        let canonical = self.canonical_collection_name(name);
        let tx = self.begin_data_readonly_tx_for(&canonical)?;
        let raw = tx
            .get(&key_encoding::sequence_key(&canonical))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(self
                .list_sequences()?
                .into_iter()
                .find(|sequence| self.canonical_collection_name(&sequence.name) == canonical));
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid sequence metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_sequences(&self) -> Result<Vec<crate::catalog::SequenceMeta>, CassieError> {
        let entries = self.scan_database_families(&key_encoding::sequence_prefix())?;
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
        let stored_name = self.get_sequence(name)?.map_or_else(
            || self.canonical_collection_name(name),
            |sequence| sequence.name,
        );
        let mut tx = self.begin_data_rw_tx_for(&stored_name)?;
        tx.delete(key_encoding::sequence_key(&stored_name))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// `nextval`: advances the stored sequence atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn next_sequence_value(&self, name: &str) -> Result<i64, CassieError> {
        self.update_sequence(name, |metadata| {
            metadata.advance().map_err(CassieError::from)
        })
        .map(|(value, _)| value)
    }

    /// Like [`Self::update_sequence`], but retries when a concurrent update to
    /// the same sequence wins the commit, so SQL callers never observe the
    /// storage write conflict that PostgreSQL's `nextval` cannot raise.
    ///
    /// # Errors
    ///
    /// Returns the update's error, or a non-retryable storage error.
    pub fn update_sequence_retrying<T>(
        &self,
        name: &str,
        mut update: impl FnMut(&mut crate::catalog::SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, crate::catalog::SequenceMeta), CassieError> {
        const MAX_ATTEMPTS: usize = 64;
        let mut attempt = 1;
        loop {
            match self.update_sequence(name, &mut update) {
                Err(CassieError::StorageRetryable(_)) if attempt < MAX_ATTEMPTS => attempt += 1,
                outcome => return outcome,
            }
        }
    }

    /// Applies `update` to the stored sequence in one database-family transaction and
    /// returns its result with the updated metadata.
    ///
    /// # Errors
    ///
    /// Returns the update's error, or a storage error.
    pub fn update_sequence<T>(
        &self,
        name: &str,
        update: impl FnOnce(&mut crate::catalog::SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, crate::catalog::SequenceMeta), CassieError> {
        let stored_name = self
            .get_sequence(name)?
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Sequence,
                name: name.to_string(),
            })?
            .name;
        let mut tx = self.begin_data_rw_tx_for(&stored_name)?;
        let raw = tx
            .get(&key_encoding::sequence_key(&stored_name))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Sequence,
                name: name.to_string(),
            })?;
        let mut metadata: crate::catalog::SequenceMeta =
            serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid sequence metadata for '{name}': {error}"))
            })?;
        let result = update(&mut metadata)?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key_encoding::sequence_key(&stored_name), value, None)
            .map_err(CassieError::from)?;
        super::schema_write_control::pause_before_schema_write_commit(
            super::schema_write_control::SchemaWritePausePoint::SequenceNextValue,
        );
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok((result, metadata))
    }
}
