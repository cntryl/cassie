use super::{
    check_projection_metadata_persistence_failure_point,
    check_projection_replay_metadata_commit_failure_point, CassieError, Midge, ProjectionMeta,
};

#[derive(Debug, Clone)]
pub(crate) struct ReplayBatchCommit {
    pub(crate) expected_metadata: Option<ProjectionMeta>,
    pub(crate) metadata: ProjectionMeta,
    pub(crate) source_identity: String,
    pub(crate) batch_id: String,
    pub(crate) observed_events: Vec<(String, bool)>,
    pub(crate) new_event_ids: Vec<String>,
}

impl Midge {
    pub(super) fn stage_projection_replay_in_tx(
        &self,
        tx: &mut super::AdapterTransaction,
        replay: &ReplayBatchCommit,
    ) -> Result<(), CassieError> {
        self.require_projection_definition(&replay.metadata)?;
        let collection = self.canonical_collection_name(&replay.metadata.collection);
        Self::assert_replay_metadata_in_tx(tx, &collection, replay.expected_metadata.as_ref())?;
        for (event_id, expected_seen) in &replay.observed_events {
            let key = Self::projection_event_key(&collection, &replay.source_identity, event_id);
            let actual = tx.get(&key).map_err(CassieError::from)?;
            if actual.is_some() != *expected_seen {
                return Err(CassieError::from(cntryl_midge::MidgeError::WriteConflict(
                    "projection replay event ledger changed during preparation".to_owned(),
                )));
            }
            tx.assert_value(key, actual.map(|bytes| bytes.to_vec()))
                .map_err(CassieError::from)?;
        }
        for event_id in &replay.new_event_ids {
            tx.put(
                Self::projection_event_key(&collection, &replay.source_identity, event_id),
                replay.batch_id.as_bytes().to_vec(),
                None,
            )
            .map_err(CassieError::from)?;
        }
        let mut metadata = replay.metadata.clone();
        metadata.collection = collection;
        Self::save_projection_metadata_to_tx(tx, &metadata)?;
        check_projection_metadata_persistence_failure_point()?;
        check_projection_replay_metadata_commit_failure_point()?;
        Ok(())
    }

    fn assert_replay_metadata_in_tx(
        tx: &mut super::AdapterTransaction,
        collection: &str,
        expected: Option<&ProjectionMeta>,
    ) -> Result<(), CassieError> {
        let metadata_key = Self::projection_key(collection);
        let stored = tx.get(&metadata_key).map_err(CassieError::from)?;
        let stored_metadata = stored
            .as_deref()
            .map(serde_json::from_slice::<ProjectionMeta>)
            .transpose()
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        let mut expected = expected.cloned();
        if let Some(metadata) = &mut expected {
            metadata.collection = collection.to_owned();
        }
        let actual_value = serde_json::to_value(&stored_metadata)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        let expected_value = serde_json::to_value(&expected)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        if actual_value != expected_value {
            return Err(CassieError::from(cntryl_midge::MidgeError::WriteConflict(
                "projection replay metadata changed during preparation".to_owned(),
            )));
        }
        tx.assert_value(metadata_key, stored.map(|bytes| bytes.to_vec()))
            .map_err(CassieError::from)
    }

    pub(crate) fn put_replay_failure_if_unchanged(
        &self,
        expected: Option<&ProjectionMeta>,
        metadata: &ProjectionMeta,
    ) -> Result<(), CassieError> {
        self.require_projection_definition(metadata)?;
        let mut metadata = metadata.clone();
        metadata.collection = self.canonical_collection_name(&metadata.collection);
        let mut tx = self.begin_data_rw_tx_for(&metadata.collection)?;
        Self::assert_replay_metadata_in_tx(&mut tx, &metadata.collection, expected)?;
        Self::save_projection_metadata_to_tx(&mut tx, &metadata)?;
        check_projection_metadata_persistence_failure_point()?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_projection_metadata(&self, metadata: &ProjectionMeta) -> Result<(), CassieError> {
        self.require_projection_definition(metadata)?;
        let mut metadata = metadata.clone();
        metadata.collection = self.canonical_collection_name(&metadata.collection);
        let mut tx = self.begin_data_rw_tx_for(&metadata.collection)?;
        Self::save_projection_metadata_to_tx(&mut tx, &metadata)?;
        check_projection_metadata_persistence_failure_point()?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_projection_metadata(&self) -> Result<Vec<ProjectionMeta>, CassieError> {
        let entries = self.scan_database_families(&Self::projection_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice::<ProjectionMeta>(&raw_value) else {
                continue;
            };
            if let Some(definition) = &record.materialized {
                crate::sql::definition_guard::version(definition.definition_semantics_version)?;
            }
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
        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        tx.delete(Self::projection_key(&collection))
            .map_err(CassieError::from)?;
        Self::delete_keys_with_prefix(&mut tx, Self::projection_event_prefix(&collection))?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
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
        let tx = self.begin_data_readonly_tx_for(&projection)?;
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
        let tx = self.begin_data_readonly_tx_for(&projection)?;
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
        let mut tx = self.begin_data_rw_tx_for(&projection)?;
        for event_id in event_ids {
            tx.put(
                Self::projection_event_key(&projection, source_identity, event_id),
                replay_batch_id.as_bytes().to_vec(),
                None,
            )
            .map_err(CassieError::from)?;
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }
}

impl Midge {
    /// Rekeys one SQL materialized-view definition in the owning database family.
    pub(crate) fn rename_sql_materialized_view_metadata(
        &self,
        previous: &str,
        metadata: &ProjectionMeta,
    ) -> Result<(), CassieError> {
        self.require_projection_definition(metadata)?;
        let previous = self.canonical_collection_name(previous);
        let mut metadata = metadata.clone();
        metadata.collection = self.canonical_collection_name(&metadata.collection);
        let mut tx = self.begin_data_rw_tx_for(&previous)?;
        if tx
            .get(&Self::projection_key(&previous))
            .map_err(CassieError::from)?
            .is_none()
        {
            return Err(CassieError::CollectionNotFound(previous));
        }
        if tx
            .get(&Self::projection_key(&metadata.collection))
            .map_err(CassieError::from)?
            .is_some()
        {
            return Err(crate::app::DiagnosticError::new(
                "42P07",
                "materialized-view destination already exists",
            )
            .into());
        }
        tx.delete(Self::projection_key(&previous))
            .map_err(CassieError::from)?;
        Self::save_projection_metadata_to_tx(&mut tx, &metadata)?;
        check_projection_metadata_persistence_failure_point()?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }
}
