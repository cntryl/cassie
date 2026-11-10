use super::{
    key_encoding, CassieError, IndexKind, Midge, RowSchema, Schema, TransactionMode, Uuid,
};

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
    fn collection(&self) -> &str {
        match &self.action {
            PendingSchemaCleanupAction::Table { table, .. }
            | PendingSchemaCleanupAction::Index { table, .. } => table,
            PendingSchemaCleanupAction::View { view } => view,
        }
    }
}

impl Midge {
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
        self.save_pending_schema_cleanup(&PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch,
            action: PendingSchemaCleanupAction::Index {
                table: table.clone(),
                index: index.to_string(),
            },
        })
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
