use super::{
    key_encoding, CassieError, IndexKind, Midge, RowSchema, Schema, TransactionMode, Uuid,
};

#[path = "schema_cleanup_guard_decode.rs"]
pub(super) mod guard_decode;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingSchemaCleanup {
    pub cleanup_id: String,
    pub blocked_by_epoch: u64,
    pub action: PendingSchemaCleanupAction,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum PendingSchemaCleanupAction {
    #[serde(rename = "DropTable")]
    Table { table: String, relation_id: u64 },
    #[serde(rename = "DropIndex")]
    Index { table: String, index: String },
    #[serde(rename = "DropView")]
    View { view: String },
}

impl PendingSchemaCleanup {
    pub(super) fn collection(&self) -> &str {
        match &self.action {
            PendingSchemaCleanupAction::Table { table, .. }
            | PendingSchemaCleanupAction::Index { table, .. } => table,
            PendingSchemaCleanupAction::View { view } => view,
        }
    }
}

impl Midge {
    /// Reports whether this thread is inside a transactional DDL scope, where
    /// pending cleanups are not completable and must wait for a later statement.
    pub(crate) fn defers_schema_cleanup(&self) -> bool {
        self.has_active_transactional_ddl_scope()
    }

    pub(crate) fn ensure_no_pending_index_cleanup(
        &self,
        collection: &str,
        index: &str,
    ) -> Result<(), CassieError> {
        self.check_pending_index_cleanup(collection, index, true)
    }

    pub(crate) fn ensure_no_persisted_index_cleanup(
        &self,
        collection: &str,
        index: &str,
    ) -> Result<(), CassieError> {
        self.check_pending_index_cleanup(collection, index, false)
    }

    fn check_pending_index_cleanup(
        &self,
        collection: &str,
        index: &str,
        include_overlay: bool,
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let database = crate::catalog::relation_database_name(&collection)
            .unwrap_or_else(|| self.default_database.clone());
        let family = self.database_family(&database)?;
        // This bypasses a retained statement snapshot while still seeding the
        // active DDL overlay, so both persisted and staged markers are visible.
        let tx = if include_overlay {
            self.begin_adapter_tx(family.id(), super::TransactionMode::ReadOnly)?
        } else {
            self.begin_raw_database_adapter_tx(&database, super::TransactionMode::ReadOnly)?
        };
        let controls = super::PublicationControlScope::current_query_controls();
        let scan = tx
            .scan(&super::Query::new().prefix(Self::schema_cleanup_prefix().into()))
            .map_err(CassieError::from)?;
        for entry in scan {
            if let Some(controls) = controls.as_ref() {
                controls.check_interruption()?;
            }
            let (_, raw) = entry.map_err(CassieError::from)?;
            let marker = guard_decode::decode::<PendingSchemaCleanup>(
                &raw,
                controls.as_ref(),
                "pending schema cleanup",
            )?;
            if let PendingSchemaCleanupAction::Index {
                table,
                index: pending,
            } = &marker.value.action
            {
                if self
                    .canonical_collection_name(table)
                    .eq_ignore_ascii_case(&collection)
                    && (guard_decode::names_match(pending, index, controls.as_ref())?
                        || guard_decode::names_match(index, pending, controls.as_ref())?)
                {
                    return Err(CassieError::Unsupported(format!(
                        "pending index cleanup prevents reusing index '{index}'"
                    )));
                }
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    #[must_use]
    pub fn scalar_index_collection_prefix_for_diagnostics(relation_id: u64) -> Vec<u8> {
        Self::scalar_index_collection_prefix(relation_id)
    }

    /// # Errors
    ///
    /// Returns an error when the index metadata is missing or invalid.
    pub fn time_series_index_prefix_for_diagnostics(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<Vec<u8>, CassieError> {
        let index = self
            .get_index(collection, index_name)?
            .ok_or_else(|| CassieError::Parse(format!("index '{index_name}' not found")))?;
        let relation_id = index.relation_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{index_name}' is missing its relation id"))
        })?;
        let index_id = index.storage_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{index_name}' is missing its storage id"))
        })?;
        Ok(Self::time_series_index_data_prefix(relation_id, index_id))
    }

    /// Returns the latest-only time-series manifest key for corruption and recovery diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error when the index metadata is missing or invalid.
    #[doc(hidden)]
    pub fn time_series_manifest_key_for_diagnostics(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<Vec<u8>, CassieError> {
        let (relation_id, index_id) =
            self.time_series_storage_ids_for_diagnostics(collection, index_name)?;
        Ok(super::key_encoding::time_series_index_manifest_key(
            relation_id,
            index_id,
        ))
    }

    /// Returns the latest-only time-series bucket-count prefix for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error when the index metadata is missing or invalid.
    #[doc(hidden)]
    pub fn time_series_bucket_count_prefix_for_diagnostics(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<Vec<u8>, CassieError> {
        let (relation_id, index_id) =
            self.time_series_storage_ids_for_diagnostics(collection, index_name)?;
        Ok(super::key_encoding::time_series_index_bucket_count_prefix(
            relation_id,
            index_id,
        ))
    }

    fn time_series_storage_ids_for_diagnostics(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(u64, u64), CassieError> {
        let index = self
            .get_index(collection, index_name)?
            .ok_or_else(|| CassieError::Parse(format!("index '{index_name}' not found")))?;
        let relation_id = index.relation_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{index_name}' is missing its relation id"))
        })?;
        let index_id = index.storage_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{index_name}' is missing its storage id"))
        })?;
        Ok((relation_id, index_id))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn defer_drop_collection(
        &self,
        table: &str,
        blocked_by_epoch: u64,
    ) -> Result<(), CassieError> {
        let table = self.canonical_collection_name(table);
        if self.collection_schema(&table).is_none() {
            return Err(CassieError::CollectionNotFound(table));
        }
        let relation_id = self.row_schema(&table)?.relation_id;
        if relation_id == 0 {
            return Err(CassieError::Unsupported(
                "deferred collection drop requires a persisted relation identity".to_string(),
            ));
        }
        self.save_pending_schema_cleanup(&PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch,
            action: PendingSchemaCleanupAction::Table {
                table: table.clone(),
                relation_id,
            },
        })?;
        if self.has_staged_transactional_ddl_scope() {
            // Free the private name for a new identity without reclaiming the
            // retired relation's physical data before its readers finish.
            self.unlink_transactional_collection_name(&table)?;
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn defer_drop_index(
        &self,
        table: &str,
        index: &str,
        blocked_by_epoch: u64,
    ) -> Result<(), CassieError> {
        let table = self.canonical_collection_name(table);
        let gate = self.collection_write_gate(&table);
        let _guard = gate.lock();
        if self.has_prepared_index_publication(&table, index)? {
            return Err(CassieError::Unsupported(
                "pending index publication requires recovery before DROP INDEX".to_owned(),
            ));
        }
        let cleanup = PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch,
            action: PendingSchemaCleanupAction::Index {
                table: table.clone(),
                index: index.to_string(),
            },
        };
        if !self.has_active_transactional_ddl_scope() {
            return self.save_pending_schema_cleanup(&cleanup);
        }

        // Keep the original metadata preimage in the same staged overlay as
        // the name-only cleanup. A later direct publisher must conflict with
        // that preimage before the cleanup can become durable.
        let _fresh_reads = super::StatementReadScope::enter(None);
        let metadata = self.get_index(&table, index)?.ok_or_else(|| {
            CassieError::Unsupported("staged index cleanup requires existing metadata".to_owned())
        })?;
        if !self
            .canonical_collection_name(&metadata.collection)
            .eq_ignore_ascii_case(&table)
            || metadata.relation_id().is_none_or(|id| id == 0)
            || metadata.storage_id().is_none_or(|id| id == 0)
        {
            return Err(CassieError::Unsupported(
                "staged index cleanup requires an exact stable index identity".to_owned(),
            ));
        }
        let index_key = Self::index_key(&metadata.collection, &metadata.name);
        let mut tx = self.begin_data_rw_tx_for(&table)?;
        let raw = tx
            .get(&index_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Unsupported(
                    "staged index cleanup requires an exact metadata key".to_owned(),
                )
            })?;
        let current: super::IndexMeta = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))?;
        if current != metadata {
            return Err(CassieError::Unsupported(
                "staged index cleanup metadata is ambiguous".to_owned(),
            ));
        }
        tx.put(index_key, raw.to_vec(), None)
            .map_err(CassieError::from)?;
        tx.put(
            Self::schema_cleanup_key(&table, &cleanup.cleanup_id),
            serde_json::to_vec(&cleanup).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn defer_drop_view(&self, view: &str, blocked_by_epoch: u64) -> Result<(), CassieError> {
        let view = self.canonical_collection_name(view);
        self.save_pending_schema_cleanup(&PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch,
            action: PendingSchemaCleanupAction::View { view: view.clone() },
        })
    }

    pub(crate) fn pending_schema_cleanups(&self) -> Result<Vec<PendingSchemaCleanup>, CassieError> {
        let entries = self.scan_database_families(&Self::schema_cleanup_prefix())?;
        let mut cleanups = Vec::new();
        for (_key, value) in entries {
            let cleanup: PendingSchemaCleanup =
                serde_json::from_slice(&value).map_err(|error| {
                    CassieError::Parse(format!("invalid pending schema cleanup: {error}"))
                })?;
            cleanups.push(cleanup);
        }
        cleanups.sort_by(|left, right| left.cleanup_id.cmp(&right.cleanup_id));
        Ok(cleanups)
    }

    pub(crate) fn complete_pending_schema_cleanup(
        &self,
        cleanup: &PendingSchemaCleanup,
    ) -> Result<(), CassieError> {
        match &cleanup.action {
            PendingSchemaCleanupAction::Table { table, relation_id } => {
                if *relation_id == 0 {
                    return Err(CassieError::Unsupported(
                        "collection cleanup requires a persisted relation identity".to_string(),
                    ));
                }
                let _fresh_reads = super::StatementReadScope::enter(None);
                let gate = self.collection_write_gate(table);
                let _guard = gate.lock();
                let current_identity = self.current_collection_relation_id_for_cleanup(table)?;
                match current_identity {
                    Some(current) if current == *relation_id => self.drop_collection(table)?,
                    Some(_) => {
                        // Shared prefixes and the current descriptor belong to
                        // the replacement, not this retired physical identity.
                        self.delete_retired_collection_data(table, *relation_id)?;
                    }
                    None => self.delete_collection_data(table, *relation_id)?,
                }
            }
            PendingSchemaCleanupAction::Index { table, index } => {
                return self.complete_current_index_cleanup(cleanup, table, index);
            }
            PendingSchemaCleanupAction::View { view } => {
                self.delete_view(view)?;
            }
        }

        let collection = cleanup.collection();
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        tx.delete(Self::schema_cleanup_key(collection, &cleanup.cleanup_id))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn current_collection_relation_id_for_cleanup(
        &self,
        collection: &str,
    ) -> Result<Option<u64>, CassieError> {
        let canonical = self.canonical_collection_name(collection);
        let tx = self.database_tx_for_collection(&canonical, TransactionMode::ReadOnly)?;
        if let Some(row_schema) = Self::load_row_schema_from_tx(&tx, &canonical)? {
            return Ok(Some(row_schema.relation_id));
        }
        let Some(raw) = tx
            .get(&Self::collection_schema_key(&canonical))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let schema: Schema = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{canonical}': {error}"))
        })?;
        Ok(Some(RowSchema::from_schema(&schema).relation_id))
    }

    fn complete_current_index_cleanup(
        &self,
        cleanup: &PendingSchemaCleanup,
        table: &str,
        index: &str,
    ) -> Result<(), CassieError> {
        if self.has_active_transactional_ddl_scope() {
            return Err(CassieError::Unsupported(
                "deferred index cleanup requires a nontransactional storage scope".to_owned(),
            ));
        }
        let _fresh_reads = super::StatementReadScope::enter(None);
        let canonical = self.canonical_collection_name(table);
        let gate = self.collection_write_gate(&canonical);
        let _guard = gate.lock();
        let database = crate::catalog::relation_database_name(&canonical)
            .unwrap_or_else(|| self.default_database.clone());
        let read =
            self.begin_raw_database_adapter_tx(&database, super::TransactionMode::ReadOnly)?;
        let key = Self::schema_cleanup_key(&canonical, &cleanup.cleanup_id);
        let Some(raw) = read.get(&key).map_err(CassieError::from)? else {
            return Ok(());
        };
        let current: PendingSchemaCleanup = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid pending schema cleanup: {error}"))
        })?;
        if &current != cleanup {
            return Ok(());
        }
        drop(read);
        if self.has_prepared_index_publication(table, index)? {
            return Err(CassieError::Unsupported(
                "ambiguous pending index cleanup and publication require manual recovery; preserve both records".to_owned(),
            ));
        }
        self.complete_drop_index_cleanup(table, index)?;
        let mut tx =
            self.begin_raw_database_adapter_tx(&database, super::TransactionMode::ReadWrite)?;
        tx.delete(key).map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    fn complete_drop_index_cleanup(&self, table: &str, index: &str) -> Result<(), CassieError> {
        let metadata = self.get_index(table, index)?;
        if let Some(metadata) = metadata.as_ref() {
            if matches!(metadata.kind, IndexKind::Vector) {
                self.delete_vector_index(&metadata.collection, &metadata.field)?;
            }
            if matches!(metadata.kind, IndexKind::Column) {
                self.delete_column_batches(&metadata.collection, &metadata.name)?;
            }
        }
        let (collection, reservation_name) = metadata.map_or_else(
            || (self.canonical_collection_name(table), index.to_string()),
            |metadata| {
                let reservation_name = metadata.unique_reservation_name().to_string();
                (metadata.collection, reservation_name)
            },
        );
        let reservation_prefix =
            key_encoding::unique_scalar_index_reservation_prefix(&collection, &reservation_name);
        self.delete_data_keys_with_prefix_in_batches(&collection, &reservation_prefix)?;
        self.delete_index(table, index)
    }

    fn save_pending_schema_cleanup(
        &self,
        cleanup: &PendingSchemaCleanup,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(cleanup.collection())?;
        if let PendingSchemaCleanupAction::Index { table, .. } = &cleanup.action {
            let canonical = self.canonical_collection_name(table);
            let mut metadata = Self::load_collection_metadata_from_tx(&tx, &canonical)?
                .ok_or_else(|| CassieError::CollectionNotFound(canonical.clone()))?;
            metadata.has_ever_had_indexes = true;
            Self::save_collection_metadata_to_tx(&mut tx, &metadata)?;
        }
        let value =
            serde_json::to_vec(cleanup).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            Self::schema_cleanup_key(cleanup.collection(), &cleanup.cleanup_id),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{key_encoding, Midge, PendingSchemaCleanup, PendingSchemaCleanupAction, Uuid};
    use crate::catalog::{IndexKind, IndexMeta};
    use crate::midge::adapter::StorageFamily;

    #[test]
    fn should_preserve_ambiguous_cleanup_and_publication_after_restart() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie_index_cleanup_pair_{}", Uuid::new_v4()));
        let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
        midge
            .create_collection(
                "cleanup_pair",
                crate::types::Schema {
                    fields: vec![crate::types::FieldSchema {
                        name: "id".to_owned(),
                        data_type: crate::types::DataType::Int,
                        nullable: false,
                    }],
                },
            )
            .expect("collection");
        let index = IndexMeta {
            definition_semantics_version: 0,
            collection: "cleanup_pair".to_owned(),
            name: "cleanup_pair_idx".to_owned(),
            field: "id".to_owned(),
            fields: vec!["id".to_owned()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        midge.put_index(&index).expect("index");
        crate::midge::adapter::set_index_publication_failure_point(true);
        let prepared = midge.put_index(&index);
        crate::midge::adapter::set_index_publication_failure_point(false);
        assert!(prepared.is_err(), "leave journal for recovery");
        let collection = midge.canonical_collection_name(&index.collection);
        let cleanup = PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch: 0,
            action: PendingSchemaCleanupAction::Index {
                table: collection,
                index: index.name.clone(),
            },
        };
        midge
            .save_pending_schema_cleanup(&cleanup)
            .expect("legacy ambiguous marker");
        drop(midge);

        // Act
        let reopened = Midge::new_strict_with_data_dir(&path).expect("reopen");
        let cleanup_result = reopened.complete_pending_schema_cleanup(&cleanup);
        let replay_result = reopened.replay_pending_index_publications();

        // Assert
        assert!(
            cleanup_result.is_err(),
            "ambiguous cleanup must be retained"
        );
        assert!(
            replay_result.is_err(),
            "ambiguous publication must be retained"
        );
        assert_eq!(
            reopened.pending_schema_cleanups().expect("markers").len(),
            1
        );
        assert_ne!(
            reopened
                .raw_scan_prefix(
                    StorageFamily::Data,
                    &key_encoding::index_publication_prefix()
                )
                .expect("journal")
                .len(),
            0
        );
        assert!(reopened
            .get_index(&index.collection, &index.name)
            .expect("read")
            .is_some());
        drop(reopened);
        std::fs::remove_dir_all(path).expect("remove fixture");
    }

    #[test]
    fn should_keep_index_name_guarded_until_every_matching_cleanup_finishes() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("create Midge");
        midge
            .create_collection(
                "cleanup_twice",
                crate::types::Schema {
                    fields: vec![crate::types::FieldSchema {
                        name: "id".to_owned(),
                        data_type: crate::types::DataType::Int,
                        nullable: false,
                    }],
                },
            )
            .expect("create collection");
        let index = IndexMeta {
            definition_semantics_version: 0,
            collection: "cleanup_twice".to_owned(),
            name: "cleanup_twice_idx".to_owned(),
            field: "id".to_owned(),
            fields: vec!["id".to_owned()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        midge.put_index(&index).expect("index");
        midge
            .defer_drop_index(&index.collection, &index.name, 0)
            .expect("first marker");
        midge
            .defer_drop_index(&index.collection, &index.name, 0)
            .expect("second marker");
        let cleanups = midge.pending_schema_cleanups().expect("markers");
        assert_eq!(cleanups.len(), 2);
        midge
            .complete_pending_schema_cleanup(&cleanups[0])
            .expect("first cleanup");

        // Act
        let rejected = midge.put_index(&index);

        // Assert
        assert!(rejected
            .expect_err("second marker retains name")
            .to_string()
            .contains("pending index cleanup"));
        midge
            .complete_pending_schema_cleanup(&cleanups[1])
            .expect("second cleanup");
        midge
            .put_index(&index)
            .expect("name is available after both cleanups");
    }

    #[test]
    fn should_use_current_index_metadata_during_cleanup_inside_a_stale_read_scope() {
        // Arrange
        let midge = std::sync::Arc::new(Midge::new_in_memory("postgres").expect("create Midge"));
        midge
            .create_collection(
                "cleanup_snapshot",
                crate::types::Schema {
                    fields: vec![crate::types::FieldSchema {
                        name: "id".to_owned(),
                        data_type: crate::types::DataType::Int,
                        nullable: false,
                    }],
                },
            )
            .expect("create collection");
        midge
            .put_document(
                "cleanup_snapshot",
                Some("row".to_owned()),
                serde_json::json!({"id": 1}),
            )
            .expect("row");
        let controls = crate::runtime::QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            crate::runtime::QueryCancellationHandle::new(),
        );
        let snapshot =
            crate::midge::adapter::StatementDataRead::capture(&midge, "postgres", &controls)
                .expect("old snapshot");
        let index = IndexMeta {
            definition_semantics_version: 0,
            collection: "cleanup_snapshot".to_owned(),
            name: "cleanup_snapshot_idx".to_owned(),
            field: "id".to_owned(),
            fields: vec!["id".to_owned()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        midge.put_index(&index).expect("index");
        let stored = midge
            .get_index(&index.collection, &index.name)
            .expect("read")
            .expect("index");
        let prefix = Midge::scalar_index_data_prefix(
            stored.relation_id().expect("relation"),
            stored.storage_id().expect("index id"),
        );
        assert_ne!(
            midge
                .raw_scan_prefix(StorageFamily::Data, &prefix)
                .expect("entries"),
            Vec::new()
        );
        midge
            .defer_drop_index(&index.collection, &index.name, 0)
            .expect("defer");
        let cleanup = midge
            .pending_schema_cleanups()
            .expect("scan")
            .pop()
            .expect("marker");

        // Act
        {
            let _scope = crate::midge::adapter::StatementReadScope::enter(Some(&snapshot));
            midge
                .complete_pending_schema_cleanup(&cleanup)
                .expect("current cleanup");
        }

        // Assert
        assert_eq!(
            midge
                .raw_scan_prefix(StorageFamily::Data, &prefix)
                .expect("entries"),
            Vec::new()
        );
        assert!(midge
            .get_index(&index.collection, &index.name)
            .expect("read")
            .is_none());
    }

    #[test]
    fn should_ignore_a_stale_cleanup_record_after_a_name_is_reused() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("create Midge");
        midge
            .create_collection(
                "cleanup_stale",
                crate::types::Schema {
                    fields: vec![crate::types::FieldSchema {
                        name: "id".to_string(),
                        data_type: crate::types::DataType::Int,
                        nullable: false,
                    }],
                },
            )
            .expect("create collection");
        let index = IndexMeta {
            definition_semantics_version: 0,
            collection: "cleanup_stale".to_string(),
            name: "cleanup_stale_idx".to_string(),
            field: "id".to_string(),
            fields: vec!["id".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        midge.put_index(&index).expect("first index");
        midge
            .defer_drop_index(&index.collection, &index.name, 0)
            .expect("defer");
        let stale = midge
            .pending_schema_cleanups()
            .expect("scan")
            .pop()
            .expect("cleanup");
        let overlay = std::sync::Arc::new(parking_lot::Mutex::new(
            crate::midge::adapter::StorageOverlay::default(),
        ));
        let scoped = {
            let _scope = midge
                .enter_transactional_ddl_scope("postgres", overlay, true)
                .expect("scope");
            midge.complete_pending_schema_cleanup(&stale)
        };
        assert!(scoped.is_err(), "cleanup cannot mix scoped and raw writes");
        assert_eq!(
            midge
                .pending_schema_cleanups()
                .expect("marker still present")
                .len(),
            1
        );
        assert!(midge
            .get_index(&index.collection, &index.name)
            .expect("read original")
            .is_some());
        midge
            .complete_pending_schema_cleanup(&stale)
            .expect("first cleanup");
        midge.put_index(&index).expect("reuse name after cleanup");

        // Act
        midge
            .complete_pending_schema_cleanup(&stale)
            .expect("stale cleanup is a no-op");

        // Assert
        assert!(midge
            .get_index(&index.collection, &index.name)
            .expect("read")
            .is_some());
    }

    #[test]
    fn should_preserve_shared_relation_zero_rows_when_reaping_a_retired_collection() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("create Midge");
        let schema = crate::types::Schema {
            fields: vec![crate::types::FieldSchema {
                name: "id".to_owned(),
                data_type: crate::types::DataType::Int,
                nullable: false,
            }],
        };
        midge
            .create_collection("legacy_zero_a", schema.clone())
            .expect("create first legacy collection");
        midge
            .create_collection("legacy_zero_b", schema)
            .expect("create second legacy collection");
        for table in ["legacy_zero_a", "legacy_zero_b"] {
            let canonical = midge.canonical_collection_name(table);
            let mut row_schema = midge.row_schema(table).expect("row schema");
            row_schema.relation_id = 0;
            midge
                .raw_put(
                    StorageFamily::Data,
                    &Midge::row_schema_key(&canonical),
                    &serde_json::to_vec(&row_schema).expect("encode row schema"),
                )
                .expect("store legacy row schema");
            midge
                .put_document(
                    table,
                    Some(format!("{table}_row")),
                    serde_json::json!({"id": 1}),
                )
                .expect("write legacy row");
        }
        let relation_zero_rows = midge
            .raw_scan_prefix(StorageFamily::Data, &Midge::row_prefix(0))
            .expect("scan shared relation-zero rows");
        assert_eq!(relation_zero_rows.len(), 2);
        assert!(midge.defer_drop_collection("legacy_zero_a", 0).is_err());
        assert!(midge.drop_collection("legacy_zero_b").is_err());
        assert!(midge.collection_schema("legacy_zero_b").is_some());
        assert_eq!(
            midge
                .pending_schema_cleanups()
                .expect("no deferred cleanup"),
            Vec::new()
        );

        let mut replacement_schema = midge.row_schema("legacy_zero_a").expect("row schema");
        replacement_schema.relation_id = 77;
        let first_table = midge.canonical_collection_name("legacy_zero_a");
        midge
            .raw_put(
                StorageFamily::Data,
                &Midge::row_schema_key(&first_table),
                &serde_json::to_vec(&replacement_schema).expect("encode replacement schema"),
            )
            .expect("store replacement identity");
        let cleanup = PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch: 0,
            action: PendingSchemaCleanupAction::Table {
                table: first_table,
                relation_id: 0,
            },
        };
        midge
            .save_pending_schema_cleanup(&cleanup)
            .expect("save legacy cleanup marker");

        // Act
        let result = midge.complete_pending_schema_cleanup(&cleanup);

        // Assert
        assert!(result.is_err(), "relation-zero cleanup must fail closed");
        assert_eq!(
            midge
                .raw_scan_prefix(StorageFamily::Data, &Midge::row_prefix(0))
                .expect("scan shared relation-zero rows")
                .len(),
            2,
        );
        assert!(midge.collection_schema("legacy_zero_a").is_some());
        assert!(midge.collection_schema("legacy_zero_b").is_some());
        assert_eq!(
            midge
                .pending_schema_cleanups()
                .expect("pending cleanups")
                .len(),
            1
        );
    }

    #[test]
    fn should_preserve_collection_data_when_cleanup_cannot_decode_current_identity() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("create Midge");
        midge
            .create_collection(
                "cleanup_unreadable",
                crate::types::Schema {
                    fields: vec![crate::types::FieldSchema {
                        name: "id".to_owned(),
                        data_type: crate::types::DataType::Int,
                        nullable: false,
                    }],
                },
            )
            .expect("create collection");
        midge
            .put_document(
                "cleanup_unreadable",
                Some("row".to_owned()),
                serde_json::json!({"id": 1}),
            )
            .expect("write row");
        let relation_id = midge
            .row_schema("cleanup_unreadable")
            .expect("row schema")
            .relation_id;
        let canonical = midge.canonical_collection_name("cleanup_unreadable");
        midge
            .raw_put(
                StorageFamily::Data,
                &Midge::doc_key(&canonical, "row"),
                b"legacy document",
            )
            .expect("seed legacy name-scoped document");
        let cleanup = PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch: 0,
            action: PendingSchemaCleanupAction::Table {
                table: canonical.clone(),
                relation_id,
            },
        };
        midge
            .save_pending_schema_cleanup(&cleanup)
            .expect("save cleanup marker");
        midge
            .raw_put(
                StorageFamily::Data,
                &Midge::row_schema_key(&canonical),
                b"\x7b",
            )
            .expect("corrupt row schema");
        midge
            .raw_put(
                StorageFamily::Data,
                &Midge::collection_schema_key(&canonical),
                b"\x7b",
            )
            .expect("corrupt collection schema");
        let row_prefix = Midge::row_prefix(relation_id);
        assert_eq!(
            midge
                .raw_scan_prefix(StorageFamily::Data, &row_prefix)
                .expect("scan rows")
                .len(),
            1,
        );

        // Act
        let result = midge.complete_pending_schema_cleanup(&cleanup);

        // Assert
        assert!(result.is_err(), "identity decode failure must propagate");
        assert_eq!(
            midge
                .raw_scan_prefix(StorageFamily::Data, &row_prefix)
                .expect("scan rows")
                .len(),
            1,
        );
        assert_eq!(
            midge
                .pending_schema_cleanups()
                .expect("pending cleanups")
                .len(),
            1
        );
        assert!(midge
            .raw_get(StorageFamily::Data, &Midge::doc_key(&canonical, "row"))
            .expect("read name-scoped document")
            .is_some());
    }

    #[test]
    fn should_store_pending_schema_cleanup_in_database_family() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie_schema_cleanup_family_{}", Uuid::new_v4()));
        let midge = Midge::new_with_data_dir(&path).expect("create Midge");
        midge
            .create_collection(
                "schema_cleanup_family",
                crate::types::Schema {
                    fields: vec![crate::types::FieldSchema {
                        name: "id".to_string(),
                        data_type: crate::types::DataType::Int,
                        nullable: false,
                    }],
                },
            )
            .expect("create collection");

        // Act
        midge
            .defer_drop_collection("schema_cleanup_family", 0)
            .expect("defer collection drop");
        let prefix = key_encoding::schema_cleanup_prefix();
        let database_entries = midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("scan database-family cleanup records");
        let global_entries = midge
            .raw_scan_prefix(StorageFamily::Schema, &prefix)
            .expect("scan global cleanup records");

        // Assert
        assert_eq!(database_entries.len(), 1);
        assert_eq!(global_entries, Vec::<(Vec<u8>, Vec<u8>)>::new());
        drop(midge);
        std::fs::remove_dir_all(path).expect("remove test store");
    }
}
