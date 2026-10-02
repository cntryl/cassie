use crate::embeddings::VectorIndexRecord;

use super::{Cassie, CassieError};

impl Cassie {
    /// Builds a vector index after embedding the rows that predate it.
    ///
    /// Rows whose source column is NULL, and rows that already carry an embedding,
    /// are left as stored. The collection write gates are held across the backfill
    /// and the index build, so no row can be written between them without an
    /// embedding the index would then miss.
    ///
    /// # Errors
    ///
    /// Returns an error when the provider rejects a source value or a write fails.
    pub(crate) fn put_vector_index_with_backfill(
        &self,
        index: &VectorIndexRecord,
        sql_index: Option<&crate::catalog::IndexMeta>,
    ) -> Result<(), CassieError> {
        let collections = self.referential_write_collections(&index.collection);
        self.midge.with_collection_write_gates(&collections, || {
            let rows = self.prepare_vector_backfill_with_held_gates(index)?;
            let options = self.document_write_options(&index.collection);
            let publication = self
                .midge
                .publish_vector_index_with_backfill(index, sql_index, rows, &options);
            let report = match publication {
                Ok(report) => report,
                Err(error) => {
                    self.runtime.invalidate_execution_result_cache();
                    if let Ok(epoch) = self.midge.data_epoch() {
                        if epoch != self.runtime.data_epoch() {
                            self.runtime.set_data_epoch(epoch);
                            let _ = crate::executor::mark_source_projections_stale_external(
                                self,
                                &index.collection,
                            );
                            let _ = self.refresh_projection_metadata(&index.collection);
                            let _ = crate::executor::sync_derived_maintenance_debt_external(
                                self,
                                &index.collection,
                            );
                        }
                    }
                    return Err(error);
                }
            };
            self.register_vector_index(index.clone());
            if let Some(metadata) = sql_index {
                self.catalog.register_index(metadata.clone());
            }
            self.runtime
                .record_projection_write_batch(index.collection.clone(), &report.stats);
            if let Some(epoch) = report.data_epoch {
                self.runtime.set_data_epoch(epoch);
            }
            let _ =
                crate::executor::sync_derived_maintenance_debt_external(self, &index.collection);
            self.refresh_document_write_metadata(&index.collection, report.row_delta, &report.stats)
        })
    }

    fn prepare_vector_backfill_with_held_gates(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<Vec<(String, serde_json::Value)>, CassieError> {
        let batch = self.new_statement_batch(None)?;
        let mut rows = Vec::new();
        let documents = self.midge.scan_documents(&index.collection)?;
        let mut validated = false;
        for document in documents {
            let Some(object) = document.payload.as_object() else {
                continue;
            };
            let has_embedding = object
                .get(&index.field)
                .is_some_and(|value| !value.is_null());
            let Some(source_value) = object
                .get(&index.source_field)
                .filter(|value| !value.is_null())
            else {
                continue;
            };
            if has_embedding {
                continue;
            }
            if !validated {
                self.validate_embedding_compatibility(index, None)?;
                validated = true;
            }
            let embedding = self.embed_source_value(index, source_value)?;
            let mut payload = document.payload.clone();
            if let Some(object) = payload.as_object_mut() {
                object.insert(index.field.clone(), embedding);
            }
            let payload = self.prepare_document_write_for_session(
                Some(batch.session()),
                &index.collection,
                payload,
                false,
                Some(&document.id),
            )?;
            self.put_prepared_document_for_session(
                Some(batch.session()),
                &index.collection,
                document.id.clone(),
                payload.clone(),
            )?;
            rows.push((document.id, payload));
        }
        Ok(rows)
    }
}
