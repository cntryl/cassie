//! Keep delayed maintenance inside its original relation's lifetime.

use std::time::Duration;

use super::{
    Cassie, CassieError, CommittedCollection, CommittedWriteBatch, QueryExecutionControls,
};

impl Cassie {
    pub(super) fn invalidate_acknowledged_copy_hints(&self, committed: &CommittedWriteBatch) {
        // A known source commit has already published its epoch. This small
        // ownership check cannot be skipped by canceled ancillary controls.
        let _catalog = self.catalog_publication.read();
        let names = committed
            .changed_collections
            .iter()
            .map(|change| change.collection.clone())
            .collect::<Vec<_>>();
        self.midge.with_collection_write_gates(&names, || {
            for changed in &committed.changed_collections {
                match self.midge.current_row_schema_identity(&changed.collection) {
                    Ok(Some(identity)) if identity == changed.relation_id => {
                        self.catalog.clear_cardinality_stats(&changed.collection);
                    }
                    Ok(_) => {}
                    Err(_) => self.runtime.record_cardinality_unavailable(),
                }
            }
        });
    }

    pub(super) fn finish_committed_collection(
        &self,
        committed: &CommittedCollection,
        controls: &QueryExecutionControls,
        copy_finish: bool,
    ) -> Result<(), CassieError> {
        // Keep the existing catalog -> sorted collection gate order. Recursive
        // reads let post-commit work finish while ordinary statement execution
        // retains its publication guard. Each acquisition remains cancellable.
        let _catalog = loop {
            controls.check_interruption()?;
            if let Some(guard) = self
                .catalog_publication
                .try_read_for(Duration::from_millis(2))
            {
                break guard;
            }
        };
        let collection = &committed.collection;
        let mut names = vec![collection.clone()];
        names.extend(
            self.catalog
                .list_rollups_for_source(collection)
                .into_iter()
                .map(|rollup| rollup.output_collection),
        );
        // Projection freshness is fenced by the source generation. Do not
        // wait for a projection's publication gate here: a concurrent refresh
        // can be building from this source while this committed write marks it
        // stale, and waiting would make the source write part of that refresh.
        self.midge
            .with_transaction_write_gates(&names, false, controls, || {
                if self.midge.current_row_schema_identity(collection)?
                    != Some(committed.relation_id)
                {
                    return Ok(());
                }
                if copy_finish {
                    self.catalog.clear_cardinality_stats(collection);
                    if self.refresh_cardinality_stats(collection).is_err() {
                        self.runtime.record_cardinality_unavailable();
                    }
                }
                let maintenance = self.midge.refresh_document_maintenance_after_commit(
                    collection,
                    committed.row_delta,
                    &committed.changed_ids,
                );
                if maintenance.is_ok() {
                    let _ = self.refresh_projection_metadata(collection);
                }
                let _ = crate::executor::refresh_rollups_for_source_external(
                    self, collection, controls,
                );
                let _ = crate::executor::mark_source_projections_stale_external(self, collection);
                let _ = crate::executor::sync_derived_maintenance_debt_external(self, collection);
                if copy_finish {
                    let time_series = self
                        .midge
                        .rebuild_time_series_indexes_for_collection(collection);
                    if time_series.is_err() {
                        self.runtime.record_storage_access("data", true, false);
                    }
                }
                Ok(())
            })
    }
}
