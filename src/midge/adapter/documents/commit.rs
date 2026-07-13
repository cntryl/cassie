use std::collections::BTreeMap;

use super::super::{CassieError, Midge};
use super::{DocumentWriteBatchOptions, DocumentWriteBatchReport};
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
                Self::stamp_normalized_vectors_generation_in_tx(
                    &mut tx, collection, generation, records,
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
                    let _ = self.complete_column_batch_maintenance(&collection, generation);
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
