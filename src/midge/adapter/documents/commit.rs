use std::collections::BTreeMap;
use std::time::Duration;

use super::super::{CassieError, Midge};
use super::{DocumentWriteBatchOptions, DocumentWriteBatchReport};
use crate::app::StorageRetryKind;
use crate::midge::adapter::maintenance::check_fulltext_maintenance_failure_point;

impl Midge {
    pub(super) fn finish_document_write_batches(
        &self,
        options: &DocumentWriteBatchOptions,
        mut tx: cntryl_midge::Transaction,
        mut reports: BTreeMap<String, DocumentWriteBatchReport>,
        changed_collections: Vec<String>,
        vector_records_by_collection: &BTreeMap<
            String,
            Vec<(String, Vec<crate::embeddings::NormalizedVectorRecord>)>,
        >,
        fulltext_indexes_by_collection: &BTreeMap<String, Vec<crate::catalog::IndexMeta>>,
    ) -> Result<BTreeMap<String, DocumentWriteBatchReport>, CassieError> {
        if changed_collections.is_empty() {
            tx.rollback().map_err(CassieError::from)?;
            return Ok(reports);
        }

        let mut generations = BTreeMap::new();
        for collection in &changed_collections {
            let generation = Self::increment_collection_generation_in_tx(&mut tx, collection)?;
            if let Some(records) = vector_records_by_collection.get(collection) {
                let row_schema = self.row_schema(collection)?;
                Self::stamp_normalized_vectors_generation_in_tx(
                    &mut tx,
                    &row_schema,
                    generation,
                    records,
                )?;
            }
            self.stamp_vector_index_states_generation_in_tx(&mut tx, collection, generation)?;
            Self::record_column_batch_maintenance_debt_in_tx(&mut tx, collection, generation)?;
            Self::record_projection_hash_maintenance_debt_in_tx(&mut tx, collection, generation)?;
            if options.record_rollup_maintenance_debt {
                Self::record_rollup_maintenance_debt_in_tx(&mut tx, collection, generation)?;
            }
            if options.record_materialized_projection_maintenance_debt {
                Self::record_materialized_projection_maintenance_debt_in_tx(
                    &mut tx, collection, generation,
                )?;
            }
            if let Some(indexes) = fulltext_indexes_by_collection.get(collection) {
                for index in indexes {
                    if check_fulltext_maintenance_failure_point().is_err() {
                        Self::record_fulltext_maintenance_debt_in_tx(
                            &mut tx,
                            collection,
                            &index.name,
                            generation,
                        )?;
                    } else {
                        self.rebuild_fulltext_index_in_tx(&mut tx, collection, index, generation)?;
                    }
                }
            }
            generations.insert(collection.clone(), generation);
        }
        let epoch = Self::increment_data_epoch_in_tx(&mut tx)?;
        if let Err(error) = super::super::check_document_write_conflict_injection() {
            tx.rollback().map_err(CassieError::from)?;
            return Err(error);
        }
        tx.commit(options.commit).map_err(CassieError::from)?;
        let mut sorted_changed_collections = changed_collections;
        sorted_changed_collections.sort();

        for collection in &sorted_changed_collections {
            let Some(report) = reports.get_mut(collection) else {
                return Err(CassieError::Execution(format!(
                    "missing write report for collection '{collection}'"
                )));
            };
            report.data_epoch = Some(epoch);
            report.stats.batch_flushes = report.stats.batch_flushes.saturating_add(1);
        }

        if options.refresh_after_commit {
            for collection in sorted_changed_collections {
                if let Some(report) = reports.get(&collection) {
                    let generation = generations[&collection];
                    let _ = self.complete_column_batch_maintenance(
                        &collection,
                        generation,
                        Some(report.changed_ids.as_slice()),
                    );
                    let _ = self.complete_projection_hash_maintenance(
                        &collection,
                        generation,
                        report.row_delta,
                    );
                    let _ = self.rebuild_time_series_indexes_for_collection(&collection);
                }
            }
        }

        Ok(reports)
    }
}

const DOCUMENT_WRITE_BATCH_ATTEMPTS: u8 = 8;
const WRITE_STALL_BACKOFF_STEP: Duration = Duration::from_millis(2);

/// Returns how long to wait before retrying a failed document write batch, or
/// `None` when the failure must be returned to the caller.
///
/// Write conflicts retry immediately against fresh state. Write stalls are
/// storage backpressure, so retries back off linearly to let flushes and
/// compaction catch up. A fenced writer is not retried: fencing means another
/// writer now owns the lease epoch, and retrying in-process cannot regain it and
/// risks acting as a stale leader.
pub(super) fn document_write_retry_delay(error: &CassieError, attempts: u8) -> Option<Duration> {
    if attempts >= DOCUMENT_WRITE_BATCH_ATTEMPTS {
        return None;
    }
    match error.storage_retry_kind()? {
        StorageRetryKind::WriteConflict => Some(Duration::ZERO),
        StorageRetryKind::WriteStall => {
            Some(WRITE_STALL_BACKOFF_STEP.saturating_mul(u32::from(attempts)))
        }
        StorageRetryKind::Fenced | StorageRetryKind::Unavailable => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{document_write_retry_delay, Duration, DOCUMENT_WRITE_BATCH_ATTEMPTS};
    use crate::app::CassieError;

    #[test]
    fn should_retry_a_write_conflict_immediately() {
        // Arrange
        let error = CassieError::from(cntryl_midge::MidgeError::WriteConflict(
            "overlap".to_string(),
        ));

        // Act
        let delay = document_write_retry_delay(&error, 1);

        // Assert
        assert_eq!(delay, Some(Duration::ZERO));
    }

    #[test]
    fn should_back_off_linearly_before_retrying_a_write_stall() {
        // Arrange
        let error = CassieError::from(cntryl_midge::MidgeError::WriteStall("full".to_string()));

        // Act
        let delay = document_write_retry_delay(&error, 3);

        // Assert
        assert_eq!(delay, Some(Duration::from_millis(6)));
    }

    #[test]
    fn should_not_retry_a_fenced_writer() {
        // Arrange
        let error = CassieError::from(cntryl_midge::MidgeError::Fenced("stale".to_string()));

        // Act
        let delay = document_write_retry_delay(&error, 1);

        // Assert
        assert_eq!(delay, None);
    }

    #[test]
    fn should_stop_retrying_after_the_attempt_limit() {
        // Arrange
        let error = CassieError::from(cntryl_midge::MidgeError::WriteStall("full".to_string()));

        // Act
        let delay = document_write_retry_delay(&error, DOCUMENT_WRITE_BATCH_ATTEMPTS);

        // Assert
        assert_eq!(delay, None);
    }
}
