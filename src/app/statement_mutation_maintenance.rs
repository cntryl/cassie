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
                Ok(())
            })
    }
}
