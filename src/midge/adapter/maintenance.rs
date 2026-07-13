use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::{
    check_column_batch_maintenance_failure_point, check_projection_hash_maintenance_failure_point,
    collect_scan, CassieError, Midge, Query, WriteOptions,
};

const COLUMN_BATCH_ARTIFACT: &str = "column_batch";
const PROJECTION_HASH_ARTIFACT: &str = "projection_hash";
pub(crate) const ROLLUP_ARTIFACT: &str = "rollup";
pub(crate) const MATERIALIZED_PROJECTION_ARTIFACT: &str = "materialized_projection";
const FULLTEXT_ARTIFACT_PREFIX: &str = "fulltext:";
static FULLTEXT_MAINTENANCE_FAILPOINT: AtomicBool = AtomicBool::new(false);

#[doc(hidden)]
pub fn set_fulltext_maintenance_failure_point(enabled: bool) {
    FULLTEXT_MAINTENANCE_FAILPOINT.store(enabled, Ordering::SeqCst);
}

pub(crate) fn check_fulltext_maintenance_failure_point() -> Result<(), CassieError> {
    if FULLTEXT_MAINTENANCE_FAILPOINT.swap(false, Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure during fulltext maintenance".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MaintenanceDebt {
    pub(crate) collection: String,
    pub(crate) artifact: String,
    pub(crate) target_generation: u64,
    pub(crate) retry_count: u32,
    pub(crate) last_error: Option<String>,
}

impl Midge {
    pub(crate) fn validate_recovery_state(&self) -> Result<(), CassieError> {
        self.pending_schema_cleanups()?;
        self.list_maintenance_debt()?;
        self.validate_pending_index_publications()
    }

    pub(super) fn record_column_batch_maintenance_debt_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        Self::record_maintenance_debt_in_tx(
            tx,
            collection,
            COLUMN_BATCH_ARTIFACT,
            target_generation,
        )
    }

    pub(super) fn record_projection_hash_maintenance_debt_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        Self::record_maintenance_debt_in_tx(
            tx,
            collection,
            PROJECTION_HASH_ARTIFACT,
            target_generation,
        )
    }

    pub(super) fn record_rollup_maintenance_debt_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        Self::record_maintenance_debt_in_tx(tx, collection, ROLLUP_ARTIFACT, target_generation)
    }

    pub(super) fn record_materialized_projection_maintenance_debt_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        Self::record_maintenance_debt_in_tx(
            tx,
            collection,
            MATERIALIZED_PROJECTION_ARTIFACT,
            target_generation,
        )
    }

    pub(super) fn record_fulltext_maintenance_debt_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        index_name: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        Self::record_maintenance_debt_in_tx(
            tx,
            collection,
            &format!("{FULLTEXT_ARTIFACT_PREFIX}{index_name}"),
            target_generation,
        )
    }

    fn record_maintenance_debt_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        artifact: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        let debt = MaintenanceDebt {
            collection: collection.to_string(),
            artifact: artifact.to_string(),
            target_generation,
            retry_count: 0,
            last_error: None,
        };
        tx.put(
            Self::maintenance_debt_key(collection, artifact),
            serde_json::to_vec(&debt).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)
    }

    pub(super) fn complete_column_batch_maintenance(
        &self,
        collection: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        let rebuild = check_column_batch_maintenance_failure_point()
            .and_then(|()| self.rebuild_column_batches_for_collection(collection));
        if let Err(error) = rebuild {
            self.record_maintenance_failure(
                collection,
                COLUMN_BATCH_ARTIFACT,
                target_generation,
                &error,
            )?;
            return Err(error);
        }
        if self.collection_generation(collection)? != target_generation {
            return Ok(());
        }
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        tx.delete(Self::maintenance_debt_key(
            collection,
            COLUMN_BATCH_ARTIFACT,
        ))
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(super) fn complete_projection_hash_maintenance(
        &self,
        collection: &str,
        target_generation: u64,
        row_delta: i64,
    ) -> Result<(), CassieError> {
        let refresh = check_projection_hash_maintenance_failure_point()
            .and_then(|()| self.refresh_projection_hashes_after_write(collection, row_delta));
        if let Err(error) = refresh {
            self.record_maintenance_failure(
                collection,
                PROJECTION_HASH_ARTIFACT,
                target_generation,
                &error,
            )?;
            return Err(error);
        }
        self.clear_maintenance_debt(collection, PROJECTION_HASH_ARTIFACT, target_generation)
    }

    fn clear_maintenance_debt(
        &self,
        collection: &str,
        artifact: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        if self.collection_generation(collection)? != target_generation {
            return Ok(());
        }
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        tx.delete(Self::maintenance_debt_key(collection, artifact))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    fn record_maintenance_failure(
        &self,
        collection: &str,
        artifact: &str,
        target_generation: u64,
        _error: &CassieError,
    ) -> Result<(), CassieError> {
        let key = Self::maintenance_debt_key(collection, artifact);
        let read_tx = self.begin_data_readonly_tx_for(collection)?;
        let retry_count = read_tx
            .get(&key)
            .map_err(CassieError::from)?
            .and_then(|raw| serde_json::from_slice::<MaintenanceDebt>(&raw).ok())
            .map_or(1, |debt| debt.retry_count.saturating_add(1));
        let debt = MaintenanceDebt {
            collection: collection.to_string(),
            artifact: artifact.to_string(),
            target_generation,
            retry_count,
            last_error: Some(format!("{artifact} maintenance failed (details redacted)")),
        };
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        tx.put(
            key,
            serde_json::to_vec(&debt)
                .map_err(|serialize| CassieError::Parse(serialize.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(crate) fn record_rollup_maintenance_failure(
        &self,
        collection: &str,
        target_generation: u64,
        error: &CassieError,
    ) -> Result<(), CassieError> {
        self.record_maintenance_failure(collection, ROLLUP_ARTIFACT, target_generation, error)
    }

    pub(crate) fn clear_rollup_maintenance_debt(
        &self,
        collection: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        self.clear_maintenance_debt(collection, ROLLUP_ARTIFACT, target_generation)
    }

    pub(crate) fn record_materialized_projection_maintenance_failure(
        &self,
        collection: &str,
        target_generation: u64,
        error: &CassieError,
    ) -> Result<(), CassieError> {
        self.record_maintenance_failure(
            collection,
            MATERIALIZED_PROJECTION_ARTIFACT,
            target_generation,
            error,
        )
    }

    pub(crate) fn clear_materialized_projection_maintenance_debt(
        &self,
        collection: &str,
        target_generation: u64,
    ) -> Result<(), CassieError> {
        self.clear_maintenance_debt(
            collection,
            MATERIALIZED_PROJECTION_ARTIFACT,
            target_generation,
        )
    }

    pub(crate) fn refresh_document_maintenance_after_commit(
        &self,
        collection: &str,
        row_delta: i64,
    ) -> Result<(), CassieError> {
        let generation = self.collection_generation(collection)?;
        let column_batches = self.complete_column_batch_maintenance(collection, generation);
        let projection_hashes =
            self.complete_projection_hash_maintenance(collection, generation, row_delta);
        column_batches.and(projection_hashes)
    }

    pub(crate) fn list_maintenance_debt(&self) -> Result<Vec<MaintenanceDebt>, CassieError> {
        let mut debts = Vec::new();
        for database in self.list_databases()? {
            let tx = self.database_tx(&database.name, cntryl_midge::TransactionMode::ReadOnly)?;
            let entries = collect_scan(
                tx.scan(&Query::new().prefix(Self::maintenance_debt_prefix().into()))
                    .map_err(CassieError::from)?,
            )?;
            for (_key, raw) in entries {
                let debt = serde_json::from_slice::<MaintenanceDebt>(&raw).map_err(|error| {
                    CassieError::Parse(format!("invalid maintenance debt: {error}"))
                })?;
                debts.push(debt);
            }
        }
        Ok(debts)
    }

    pub(crate) fn maintenance_debt_for(
        &self,
        collection: &str,
        artifact: &str,
    ) -> Result<Option<MaintenanceDebt>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::maintenance_debt_key(&collection, artifact))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid maintenance debt: {error}")))
    }

    /// Retries persisted rebuildable-artifact work before catalog hydration.
    ///
    /// # Errors
    ///
    /// Returns an error when maintenance debt cannot be read or its collection generation cannot
    /// be loaded. Individual rebuild failures remain durable debt for a later retry.
    pub fn retry_maintenance_debt(&self) -> Result<(), CassieError> {
        let mut debts = Vec::new();
        for database in self.list_databases()? {
            let tx = self.database_tx(&database.name, cntryl_midge::TransactionMode::ReadOnly)?;
            let entries = collect_scan(
                tx.scan(&Query::new().prefix(Self::maintenance_debt_prefix().into()))
                    .map_err(CassieError::from)?,
            )?;
            debts.extend(
                entries
                    .into_iter()
                    .filter_map(|(_, raw)| serde_json::from_slice::<MaintenanceDebt>(&raw).ok()),
            );
        }
        for debt in debts {
            if debt.artifact == COLUMN_BATCH_ARTIFACT {
                let generation = self.collection_generation(&debt.collection)?;
                let _ = self.complete_column_batch_maintenance(&debt.collection, generation);
            } else if debt.artifact == PROJECTION_HASH_ARTIFACT {
                let generation = self.collection_generation(&debt.collection)?;
                if self.rebuild_projection_hashes(&debt.collection).is_ok() {
                    let _ = self.clear_maintenance_debt(
                        &debt.collection,
                        PROJECTION_HASH_ARTIFACT,
                        generation,
                    );
                }
            } else if let Some(index_name) = debt.artifact.strip_prefix(FULLTEXT_ARTIFACT_PREFIX) {
                let generation = self.collection_generation(&debt.collection)?;
                let index = self.list_indexes()?.into_iter().find(|index| {
                    index.collection == debt.collection
                        && index.name == index_name
                        && index.kind == crate::catalog::IndexKind::FullText
                });
                if let Some(index) = index {
                    if self.rebuild_fulltext_index_for_index(&index).is_ok() {
                        let _ = self.clear_maintenance_debt(
                            &debt.collection,
                            &debt.artifact,
                            generation,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn has_column_batch_maintenance_debt(&self, collection: &str) -> Result<bool, CassieError> {
        self.has_maintenance_debt(collection, COLUMN_BATCH_ARTIFACT)
    }

    #[doc(hidden)]
    pub fn has_projection_hash_maintenance_debt(
        &self,
        collection: &str,
    ) -> Result<bool, CassieError> {
        self.has_maintenance_debt(collection, PROJECTION_HASH_ARTIFACT)
    }

    #[doc(hidden)]
    pub fn has_rollup_maintenance_debt(&self, collection: &str) -> Result<bool, CassieError> {
        self.has_maintenance_debt(collection, ROLLUP_ARTIFACT)
    }

    #[doc(hidden)]
    pub fn has_materialized_projection_maintenance_debt(
        &self,
        collection: &str,
    ) -> Result<bool, CassieError> {
        self.has_maintenance_debt(collection, MATERIALIZED_PROJECTION_ARTIFACT)
    }

    #[doc(hidden)]
    pub fn has_fulltext_maintenance_debt(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<bool, CassieError> {
        self.has_maintenance_debt(
            collection,
            &format!("{FULLTEXT_ARTIFACT_PREFIX}{index_name}"),
        )
    }

    fn has_maintenance_debt(&self, collection: &str, artifact: &str) -> Result<bool, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        tx.get(&Self::maintenance_debt_key(&collection, artifact))
            .map(|debt| debt.is_some())
            .map_err(CassieError::from)
    }
}
