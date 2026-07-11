use serde::{Deserialize, Serialize};

use super::{
    check_column_batch_maintenance_failure_point, check_projection_hash_maintenance_failure_point,
    CassieError, Midge, Query, WriteOptions,
};

const COLUMN_BATCH_ARTIFACT: &str = "column_batch";
const PROJECTION_HASH_ARTIFACT: &str = "projection_hash";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct MaintenanceDebt {
    collection: String,
    artifact: String,
    target_generation: u64,
    retry_count: u32,
    last_error: Option<String>,
}

impl Midge {
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
        let mut tx = self.begin_data_rw_tx()?;
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
        let mut tx = self.begin_data_rw_tx()?;
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
        let read_tx = self.begin_data_readonly_tx()?;
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
        let mut tx = self.begin_data_rw_tx()?;
        tx.put(
            key,
            serde_json::to_vec(&debt)
                .map_err(|serialize| CassieError::Parse(serialize.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    /// Retries persisted rebuildable-artifact work before catalog hydration.
    ///
    /// # Errors
    ///
    /// Returns an error when maintenance debt cannot be read or its collection generation cannot
    /// be loaded. Individual rebuild failures remain durable debt for a later retry.
    pub fn retry_maintenance_debt(&self) -> Result<(), CassieError> {
        let tx = self.begin_data_readonly_tx()?;
        let entries = tx
            .scan(&Query::new().prefix(Self::maintenance_debt_prefix().into()))
            .map_err(CassieError::from)?;
        let debts = entries
            .into_iter()
            .filter_map(|(_, raw)| serde_json::from_slice::<MaintenanceDebt>(&raw).ok())
            .collect::<Vec<_>>();
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
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn has_projection_hash_maintenance_debt(
        &self,
        collection: &str,
    ) -> Result<bool, CassieError> {
        let tx = self.begin_data_readonly_tx()?;
        tx.get(&Self::maintenance_debt_key(
            collection,
            PROJECTION_HASH_ARTIFACT,
        ))
        .map(|debt| debt.is_some())
        .map_err(CassieError::from)
    }
}
