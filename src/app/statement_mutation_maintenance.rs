//! Keep delayed maintenance inside its original relation's lifetime.

use std::time::Duration;

use super::{Cassie, CassieError, CommittedCollection, QueryExecutionControls};

impl Cassie {
    pub(super) fn finish_committed_collection(
        &self,
        committed: CommittedCollection,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        // Keep the existing catalog -> sorted collection gate order. Recursive
        // reads also work when ordinary statement execution retains its guard.
        let _catalog = loop {
            controls.check_interruption()?;
            if let Some(guard) = self
                .catalog_publication
                .try_read_recursive_for(Duration::from_millis(2))
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
        names.extend(
            self.catalog
                .list_projection_metadata()
                .into_iter()
                .filter(|projection| {
                    projection.materialized.as_ref().is_some_and(|materialized| {
                        materialized
                            .source_collections
                            .iter()
                            .any(|source| source.eq_ignore_ascii_case(collection))
                    })
                })
                .map(|projection| projection.collection),
        );
        self.midge.with_transaction_write_gates(&names, false, controls, || {
            if self.midge.current_row_schema_identity(collection)? != Some(committed.relation_id) {
                return Ok(());
            }
            let maintenance = self.midge.refresh_document_maintenance_after_commit(
                collection,
                committed.row_delta,
                &committed.changed_ids,
            );
            if maintenance.is_ok() {
                let _ = self.refresh_projection_metadata(collection);
            }
            let _ = crate::executor::refresh_rollups_for_source_external(self, collection, controls);
            let _ = crate::executor::mark_source_projections_stale_external(self, collection);
            let _ = crate::executor::sync_derived_maintenance_debt_external(self, collection);
            Ok(())
        })
    }
}
