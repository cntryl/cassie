use std::collections::BTreeMap;
use std::time::Duration;

use super::super::{CassieError, Midge};
use super::{DocumentWriteBatchOptions, DocumentWriteBatchReport};
use crate::adapter::StorageRetryKind;
use crate::adapter::fulltext_retrieval::{FulltextCommitChanges, FulltextWriteTally};
use crate::adapter::maintenance::check_fulltext_maintenance_failure_point;
use crate::adapter::vector_indexes::{VectorBatchWork, VectorFieldWrites};

impl Midge {
    /// Maintains each full-text index of a changed collection, or records
    /// durable debt when the maintenance failpoint is armed.
    fn maintain_fulltext_indexes_in_tx(
        &self,
        tx: &mutcrate::adapter::AdapterTransaction,
        collection: &str,
        indexes: &[crate::catalog::IndexMeta],
        generation: u64,
        report: Option<&DocumentWriteBatchReport>,
        deferred_rebuilds: &mut Vec<(String, crate::catalog::IndexMeta, u64)>,
    ) -> Result<Vec<FulltextWriteTally>, CassieError> {
        let mut tallies = Vec::new();
        for index in indexes {
            if check_fulltext_maintenance_failure_point().is_err() {
                Self::record_fulltext_maintenance_debt_in_tx(
                    tx,
                    collection,
                    &index.name,
                    generation,
                )?;
            } else {
                let (tally, deferred) = self.maintain_fulltext_index_in_tx(
                    tx,
                    collection,
                    index,
                    generation,
                    &FulltextCommitChanges {
                        changed_ids: report.map_or(&[][..], |report| report.changed_ids.as_slice()),
                        row_delta: report.map_or(0, |report| report.row_delta),
                    },
                )?;
                if deferred {
                    deferred_rebuilds.push((collection.to_string(), index.clone(), generation));
                } else {
                    tallies.push(tally);
                }
            }
        }
        Ok(tallies)
    }

    /// Records a durable commit's maintenance tallies, then rebuilds the indexes
    /// the commit unpublished because they were too large to rebuild inside it.
    fn publish_fulltext_maintenance(
        &self,
        tallies: &[FulltextWriteTally],
        deferred_rebuilds: &[(String, crate::catalog::IndexMeta, u64)],
    ) {
        for tally in tallies {
            self.fulltext_maintenance_metrics.record(tally);
        }
        for (collection, index, generation) in deferred_rebuilds {
            self.repair_fulltext_maintenance_debt(collection, index, *generation);
        }
    }

    fn stamp_vector_generations_in_tx(
        &self,
        tx: &mutcrate::adapter::AdapterTransaction,
        collection: &str,
        generation: u64,
        writes: Option<&Vec<VectorFieldWrites>>,
    ) -> Result<(), CassieError> {
        if let Some(writes) = writes {
            let row_schema = self.row_schema(collection)?;
            Self::stamp_normalized_vectors_generation_in_tx(tx, &row_schema, generation, writes)?;
        }
        self.stamp_vector_index_states_generation_in_tx(tx, collection, generation)
    }

    /// Test boundary at which index maintenance is staged but not committed.
    fn pause_before_index_publication(vector: &VectorBatchWork, fulltext_changed: bool) {
        use super::super::schema_write_control::{
            pause_before_schema_write_commit, SchemaWritePausePoint,
        };
        if vector
            .writes_by_collection
            .values()
            .any(|fields| !fields.is_empty())
        {
            pause_before_schema_write_commit(SchemaWritePausePoint::VectorIndexPublication);
        }
        if fulltext_changed {
            pause_before_schema_write_commit(SchemaWritePausePoint::FulltextPostingPublication);
        }
    }

    pub(super) fn finish_document_write_batches(
        &self,
        options: &DocumentWriteBatchOptions,
        mut tx:crate::adapter::AdapterTransaction,
        mut reports: BTreeMap<String, DocumentWriteBatchReport>,
        changed_collections: Vec<String>,
        vector: &VectorBatchWork,
        fulltext_indexes_by_collection: &BTreeMap<String, Vec<crate::catalog::IndexMeta>>,
    ) -> Result<BTreeMap<String, DocumentWriteBatchReport>, CassieError> {
        if changed_collections.is_empty() && !options.has_side_writes() {
            tx.rollback().map_err(CassieError::from)?;
            return Ok(reports);
        }

        let mut generations = BTreeMap::new();
        let mut fulltext_tallies = Vec::<FulltextWriteTally>::new();
        let mut deferred_fulltext_rebuilds = Vec::new();
        for collection in &changed_collections {
            let generation = Self::increment_collection_generation_in_tx(&mut tx, collection)?;
            self.stamp_vector_generations_in_tx(
                &mut tx,
                collection,
                generation,
                vector.writes_by_collection.get(collection),
            )?;
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
                fulltext_tallies.extend(self.maintain_fulltext_indexes_in_tx(
                    &mut tx,
                    collection,
                    indexes,
                    generation,
                    reports.get(collection),
                    &mut deferred_fulltext_rebuilds,
                )?);
            }
            generations.insert(collection.clone(), generation);
        }
        let epoch = if changed_collections.is_empty() {
            None
        } else {
            Some(Self::increment_data_epoch_in_tx(&mut tx)?)
        };
        if let Some((key, value)) = &options.publication_marker {
            tx.put(key.clone(), value.clone(), None)
                .map_err(CassieError::from)?;
        }
        options.stage_additional_data_writes(&mut tx)?;
        if let Err(error) = super::super::check_document_write_storage_failure_injection() {
            tx.rollback().map_err(CassieError::from)?;
            return Err(error);
        }
        if options.replay.is_some() {
            super::super::schema_write_control::pause_before_schema_write_commit(
                super::super::schema_write_control::SchemaWritePausePoint::ProjectionReplayMetadataCommit,
            );
        }
        Self::pause_before_index_publication(vector, !fulltext_tallies.is_empty());
        tx.commit(options.commit).map_err(CassieError::from)?;
        self.publish_fulltext_maintenance(&fulltext_tallies, &deferred_fulltext_rebuilds);
        self.publish_vector_maintenance(&vector.maintenance);
        let mut sorted_changed_collections = changed_collections;
        sorted_changed_collections.sort();

        for collection in &sorted_changed_collections {
            let Some(report) = reports.get_mut(collection) else {
                if options.replay.is_some() {
                    return Err(CassieError::from(cntryl_midge::MidgeError::OutcomeUnknown(
                        format!("replay committed but write report for '{collection}' is missing"),
                    )));
                }
                return Err(CassieError::Execution(format!(
                    "missing write report for collection '{collection}'"
                )));
            };
            report.data_epoch = epoch;
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
                    // Time-series index entries, bucket counts and the manifest
                    // are maintained inside the write transaction itself, so no
                    // post-commit full-index rebuild runs here. That rebuild
                    // rescanned the whole relation after every batch.
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
/// risks acting as a stale leader. Unknown commit outcomes are terminal and
/// require reconciliation after recovery before the caller considers a retry.
pub(incrate::adapter) fn document_write_retry_delay(
    error: &CassieError,
    attempts: u8,
) -> Option<Duration> {
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
    use crate::adapter::CassieError;

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

#[cfg(test)]
mod outcome_tests {
    use super::document_write_retry_delay;
    use crate::adapter::CassieError;

    #[test]
    fn should_not_retry_an_unknown_commit_given_conflict_text_when_write_retry_is_considered() {
        // Arrange
        let error = CassieError::from(cntryl_midge::MidgeError::OutcomeUnknown(
            "write conflict while confirming publication".to_owned(),
        ));
        // Act
        let delay = document_write_retry_delay(&error, 1);
        // Assert
        assert_eq!(delay, None);
    }
}
