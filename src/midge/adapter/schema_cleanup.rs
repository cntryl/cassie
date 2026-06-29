use super::{Midge, CassieError, Uuid, StorageFamily, WriteOptions, IndexKind};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingSchemaCleanup {
    pub cleanup_id: String,
    pub blocked_by_epoch: u64,
    pub action: PendingSchemaCleanupAction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PendingSchemaCleanupAction {
    #[serde(rename = "DropTable")]
    Table { table: String },
    #[serde(rename = "DropIndex")]
    Index { table: String, index: String },
    #[serde(rename = "DropView")]
    View { view: String },
}

impl Midge {
    #[doc(hidden)]
    #[must_use]
    pub fn scalar_index_collection_prefix_for_diagnostics(collection: &str) -> Vec<u8> {
        Self::scalar_index_collection_prefix(collection)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn defer_drop_collection(
        &self,
        table: &str,
        blocked_by_epoch: u64,
    ) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        if tx
            .get(&Self::collection_schema_key(table))
            .map_err(CassieError::from)?
            .is_none()
        {
            return Err(CassieError::CollectionNotFound(table.to_string()));
        }
        drop(tx);
        self.save_pending_schema_cleanup(PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch,
            action: PendingSchemaCleanupAction::Table {
                table: table.to_string(),
            },
        })
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
        self.save_pending_schema_cleanup(PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch,
            action: PendingSchemaCleanupAction::Index {
                table: table.to_string(),
                index: index.to_string(),
            },
        })
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn defer_drop_view(&self, view: &str, blocked_by_epoch: u64) -> Result<(), CassieError> {
        self.save_pending_schema_cleanup(PendingSchemaCleanup {
            cleanup_id: Uuid::new_v4().to_string(),
            blocked_by_epoch,
            action: PendingSchemaCleanupAction::View {
                view: view.to_string(),
            },
        })
    }

    pub(crate) fn pending_schema_cleanups(&self) -> Result<Vec<PendingSchemaCleanup>, CassieError> {
        let entries =
            self.raw_scan_prefix(StorageFamily::Schema, &Self::schema_cleanup_prefix())?;
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
            PendingSchemaCleanupAction::Table { table } => {
                self.drop_collection(table)?;
            }
            PendingSchemaCleanupAction::Index { table, index } => {
                self.complete_drop_index_cleanup(table, index)?;
            }
            PendingSchemaCleanupAction::View { view } => {
                self.delete_view(view)?;
            }
        }

        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::schema_cleanup_key(&cleanup.cleanup_id))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    fn complete_drop_index_cleanup(&self, table: &str, index: &str) -> Result<(), CassieError> {
        if let Some(metadata) = self.get_index(table, index)? {
            if matches!(metadata.kind, IndexKind::Vector) {
                self.delete_vector_index(table, &metadata.field)?;
            }
            if matches!(metadata.kind, IndexKind::Column) {
                self.delete_column_batches(table, &metadata.name)?;
            }
        }
        self.delete_index(table, index)
    }

    fn save_pending_schema_cleanup(
        &self,
        cleanup: PendingSchemaCleanup,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&cleanup).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::schema_cleanup_key(&cleanup.cleanup_id), value, None)
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }
}
