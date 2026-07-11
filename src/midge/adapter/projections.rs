use super::{CassieError, Midge, ProjectionMeta, StorageFamily, WriteOptions};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_projection_metadata(&self, metadata: &ProjectionMeta) -> Result<(), CassieError> {
        let mut metadata = metadata.clone();
        metadata.collection = self.canonical_collection_name(&metadata.collection);
        let mut tx = self.begin_schema_rw_tx()?;
        Self::save_projection_metadata_to_tx(&mut tx, &metadata)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_projection_metadata(&self) -> Result<Vec<ProjectionMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::projection_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata: &ProjectionMeta| metadata.collection.to_ascii_lowercase());
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_projection_metadata(&self, collection: &str) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::projection_key(&collection))
            .map_err(CassieError::from)?;
        Self::delete_keys_with_prefix(&mut tx, Self::projection_event_prefix(&collection))?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn has_projection_event(
        &self,
        projection: &str,
        source_identity: &str,
        event_id: &str,
    ) -> Result<bool, CassieError> {
        let projection = self.canonical_collection_name(projection);
        let tx = self.begin_schema_readonly_tx()?;
        tx.get(&Self::projection_event_key(
            &projection,
            source_identity,
            event_id,
        ))
        .map(|value| value.is_some())
        .map_err(CassieError::from)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn projection_events_seen(
        &self,
        projection: &str,
        source_identity: &str,
        event_ids: &[&str],
    ) -> Result<Vec<bool>, CassieError> {
        let projection = self.canonical_collection_name(projection);
        let tx = self.begin_schema_readonly_tx()?;
        let mut out = Vec::with_capacity(event_ids.len());
        for event_id in event_ids {
            let seen = tx
                .get(&Self::projection_event_key(
                    &projection,
                    source_identity,
                    event_id,
                ))
                .map_err(CassieError::from)?
                .is_some();
            out.push(seen);
        }
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn record_projection_event(
        &self,
        projection: &str,
        source_identity: &str,
        event_id: &str,
        replay_batch_id: &str,
    ) -> Result<(), CassieError> {
        let projection = self.canonical_collection_name(projection);
        self.record_projection_events_batch(
            &projection,
            source_identity,
            &[event_id],
            replay_batch_id,
        )
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn record_projection_events_batch(
        &self,
        projection: &str,
        source_identity: &str,
        event_ids: &[&str],
        replay_batch_id: &str,
    ) -> Result<(), CassieError> {
        let projection = self.canonical_collection_name(projection);
        let mut tx = self.begin_schema_rw_tx()?;
        for event_id in event_ids {
            tx.put(
                Self::projection_event_key(&projection, source_identity, event_id),
                replay_batch_id.as_bytes().to_vec(),
                None,
            )
            .map_err(CassieError::from)?;
        }
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }
}
