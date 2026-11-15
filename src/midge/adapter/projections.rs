use super::{
    check_projection_metadata_persistence_failure_point,
    check_projection_replay_metadata_commit_failure_point, CassieError, Midge, ProjectionMeta,
};

#[derive(serde::Deserialize)]
pub(crate) struct ProjectionCheckpoint {
    #[serde(default)]
    pub(crate) projection_id: String,
    pub(crate) collection: String,
    #[serde(default)]
    pub(crate) kind: crate::catalog::ProjectionKind,
    #[serde(default)]
    pub(crate) source_identity: Option<String>,
    #[serde(default)]
    pub(crate) source_checkpoint: Option<String>,
    #[serde(default)]
    pub(crate) source_position: Option<u64>,
    #[serde(default)]
    pub(crate) last_applied_event_id: Option<String>,
    #[serde(default)]
    pub(crate) replay_batch_id: Option<String>,
    #[serde(default)]
    pub(crate) lag: u64,
    #[serde(default)]
    pub(crate) freshness: crate::catalog::ProjectionFreshness,
    #[serde(default)]
    pub(crate) last_error: Option<String>,
    #[serde(default)]
    materialized: Option<CheckpointDefinition>,
}

#[derive(serde::Deserialize)]
struct CheckpointDefinition {
    #[serde(default)]
    definition_semantics_version: u32,
}

enum CheckpointValue<'a> {
    Stored(bytes::Bytes),
    Staged(&'a [u8]),
}

impl AsRef<[u8]> for CheckpointValue<'_> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Stored(bytes) => bytes,
            Self::Staged(bytes) => bytes,
        }
    }
}

fn checkpoint_value<'a>(
    tx: &super::DataReadTransaction,
    overlay: Option<&'a super::CheckpointStorageSnapshot>,
    key: &[u8],
) -> Result<Option<CheckpointValue<'a>>, CassieError> {
    match overlay.and_then(|owner| owner.get(key)) {
        Some(super::StagedCheckpointChange::Value(value)) => {
            return Ok(Some(CheckpointValue::Staged(value)));
        }
        Some(super::StagedCheckpointChange::Deleted) => return Ok(None),
        None => {}
    }
    Ok(tx
        .get(key)
        .map_err(CassieError::from)?
        .map(CheckpointValue::Stored))
}

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
    pub(crate) fn checkpoint_rows_for_statement(
        &self,
        database: &str,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<Vec<(ProjectionCheckpoint, crate::runtime::QueryMemoryReservation)>, CassieError>
    {
        controls.check_interruption()?;
        let tx = self.begin_database_readonly_tx(database)?;
        let overlay = tx.checkpoint_storage().map(AsRef::as_ref);
        let directory_key = Self::collections_key();
        let directory = checkpoint_value(&tx, overlay, &directory_key)?;
        let directory_bytes = directory.as_ref().map_or(0, |value| value.as_ref().len());
        let directory_reservation = directory_bytes
            .checked_mul(32)
            .and_then(|bytes| bytes.checked_add(2048))
            .ok_or_else(|| {
                CassieError::ResourceLimit("checkpoint directory accounting overflow".to_owned())
            })?;
        let _directory_memory = controls.reserve_query_memory(directory_reservation)?;
        let mut collections: Vec<String> = match directory {
            Some(value) => serde_json::from_slice(value.as_ref()).map_err(|error| {
                CassieError::Parse(format!("invalid collection directory: {error}"))
            })?,
            None => Vec::new(),
        };
        collections.sort_unstable();
        if collections.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(CassieError::Storage("duplicate collection identity in checkpoint directory; repair or rebuild from the event stream".to_owned()));
        }
        for collection in &collections {
            controls.check_interruption()?;
            let key = Self::projection_key(collection);
            if checkpoint_value(&tx, overlay, &key)?.is_none() {
                return Err(CassieError::Storage(format!("projection checkpoint metadata missing for '{collection}'; repair or rebuild from the event stream")));
            }
        }
        let prefix = Self::projection_prefix();
        let _prefix_memory = controls.reserve_query_memory(prefix.len())?;
        let query = super::Query::new().prefix(prefix.clone().into());
        let scan = tx.scan(&query).map_err(CassieError::from)?;
        let mut output = Vec::new();
        for entry in scan {
            controls.check_interruption()?;
            let (key, value) = entry.map_err(CassieError::from)?;
            self.record_query_scan_entry();
            if super::query_scan_control::should_cancel_controlled_query_scan(controls) {
                return Err(CassieError::QueryCanceled);
            }
            let selected = match overlay.and_then(|owner| owner.get(&key)) {
                Some(super::StagedCheckpointChange::Value(staged)) => Some(staged),
                Some(super::StagedCheckpointChange::Deleted) => None,
                None => Some(value.as_ref()),
            };
            if let Some(value) = selected {
                Self::append_checkpoint_row(
                    &mut output,
                    &collections,
                    database,
                    &key,
                    value,
                    controls,
                )?;
            }
        }
        if let Some(overlay) = overlay {
            for (key, value) in overlay.entries() {
                controls.check_interruption()?;
                if !key.starts_with(&prefix)
                    || value.is_none()
                    || tx.get(key).map_err(CassieError::from)?.is_some()
                {
                    continue;
                }
                if let Some(value) = value {
                    Self::append_checkpoint_row(
                        &mut output,
                        &collections,
                        database,
                        key,
                        value,
                        controls,
                    )?;
                }
            }
        }
        Ok(output)
    }

    fn append_checkpoint_row(
        output: &mut Vec<(ProjectionCheckpoint, crate::runtime::QueryMemoryReservation)>,
        collections: &[String],
        database: &str,
        key: &[u8],
        value: &[u8],
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        let bytes = value
            .len()
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or_else(|| {
                CassieError::ResourceLimit("checkpoint row accounting overflow".to_owned())
            })?;
        let memory = controls.reserve_query_memory(bytes)?;
        let entry: ProjectionCheckpoint = serde_json::from_slice(value).map_err(|error| {
            CassieError::Parse(format!(
                "invalid projection checkpoint metadata; repair or rebuild: {error}"
            ))
        })?;
        if let Some(definition) = &entry.materialized {
            crate::sql::definition_guard::version(definition.definition_semantics_version)?;
        }
        if !crate::catalog::relation_belongs_to_database(&entry.collection, database) {
            return Ok(());
        }
        if Self::projection_key(&entry.collection) != key {
            return Err(CassieError::Storage("projection checkpoint key and identity disagree; repair or rebuild from the event stream".to_owned()));
        }
        if entry.kind == crate::catalog::ProjectionKind::Collection
            && collections.binary_search(&entry.collection).is_err()
        {
            return Err(CassieError::Storage(format!(
                "orphan projection checkpoint for '{}'; repair or rebuild from the event stream",
                entry.collection
            )));
        }
        output
            .try_reserve(1)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        output.push((entry, memory));
        Ok(())
    }

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
            collection.clone_into(&mut metadata.collection);
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

#[cfg(test)]
mod checkpoint_snapshot_tests {
    use super::Midge;
    use crate::app::CassieError;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::QueryExecutionControls;
    use crate::types::Schema;
    use std::time::Instant;

    fn fixture() -> Midge {
        let midge = Midge::new_in_memory("postgres").unwrap();
        midge
            .create_collection("checkpoint_fixture", Schema { fields: vec![] })
            .unwrap();
        midge
    }

    #[test]
    fn should_fail_closed_for_missing_checkpoint_marker() {
        // Arrange
        let midge = fixture();
        let collection = midge.canonical_collection_name("checkpoint_fixture");
        let mut tx = midge.begin_data_rw_tx_for(&collection).unwrap();
        tx.delete(Midge::projection_key(&collection)).unwrap();
        tx.commit(midge.write_options_sync()).unwrap();
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

        // Act
        let result = midge.checkpoint_rows_for_statement("postgres", &controls);

        // Assert
        assert!(
            matches!(result, Err(CassieError::Storage(ref message)) if message.contains("repair or rebuild"))
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_fail_closed_when_checkpoint_key_and_identity_disagree() {
        // Arrange
        let midge = fixture();
        let collection = midge.canonical_collection_name("checkpoint_fixture");
        let other = "postgres.public.\"other\"".to_string();
        let mismatched =
            serde_json::to_vec(&crate::catalog::ProjectionMeta::new(&other, 1)).unwrap();
        let mut tx = midge.begin_data_rw_tx_for(&collection).unwrap();
        tx.put(Midge::projection_key(&collection), mismatched, None)
            .unwrap();
        tx.commit(midge.write_options_sync()).unwrap();
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

        // Act
        let result = midge.checkpoint_rows_for_statement("postgres", &controls);

        // Assert
        assert!(
            matches!(result, Err(CassieError::Storage(ref message)) if message.contains("disagree"))
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_enforce_checkpoint_scan_memory_budget() {
        // Arrange
        let midge = fixture();
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());

        // Act
        let result = midge.checkpoint_rows_for_statement("postgres", &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_cancel_checkpoint_scan_at_entry_boundary() {
        // Arrange
        let _guard = super::super::query_scan_control::query_scan_control_test_guard();
        let midge = fixture();
        super::super::query_scan_control::set_query_scan_cancellation_after_entries(Some(1));
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

        // Act
        let result = midge.checkpoint_rows_for_statement("postgres", &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_preserve_exact_quoted_checkpoint_identities() {
        // Arrange
        let midge = fixture();
        let mut tx = midge.begin_data_rw_tx_for("checkpoint_fixture").unwrap();
        let mut names = Midge::load_collections(&tx).unwrap();
        let first = "postgres.public.\"Foo\"".to_string();
        let second = "postgres.public.\"FOO\"".to_string();
        names.extend([first.clone(), second.clone()]);
        Midge::save_collections(&mut tx, &names).unwrap();
        Midge::save_projection_metadata_to_tx(
            &mut tx,
            &crate::catalog::ProjectionMeta::new(&first, 1),
        )
        .unwrap();
        Midge::save_projection_metadata_to_tx(
            &mut tx,
            &crate::catalog::ProjectionMeta::new(&second, 1),
        )
        .unwrap();
        tx.commit(midge.write_options_sync()).unwrap();
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

        // Act
        let rows = midge
            .checkpoint_rows_for_statement("postgres", &controls)
            .unwrap();

        // Assert
        assert!(rows.iter().any(|(row, _)| row.collection == first));
        assert!(rows.iter().any(|(row, _)| row.collection == second));
    }
}
