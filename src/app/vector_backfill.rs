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
    ) -> Result<(), CassieError> {
        let collections = self.referential_write_collections(&index.collection);
        self.midge.with_collection_write_gates(&collections, || {
            self.backfill_vector_embeddings_with_held_gates(index)?;
            self.midge.put_vector_index(index.clone())
        })
    }

    fn backfill_vector_embeddings_with_held_gates(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<(), CassieError> {
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
            self.put_prepared_document_for_session(None, &index.collection, document.id, payload)?;
        }
        Ok(())
    }
}
