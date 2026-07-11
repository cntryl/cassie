use cntryl_midge::WriteOptions;
use uuid::Uuid;

use super::{encode_row, CassieError, Midge};

impl Midge {
    /// Load documents into a newly-created row-store collection without replacement checks.
    ///
    /// Callers must only use this for fresh collections with no secondary indexes. The loader still
    /// validates documents, writes row blobs, and updates row hashes.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_fresh_documents(
        &self,
        collection: &str,
        documents: Vec<(Option<String>, serde_json::Value)>,
    ) -> Result<Vec<String>, CassieError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        if self.collection_uses_column_store(collection)? {
            return Err(CassieError::Unsupported(
                "fresh document load requires row storage".to_string(),
            ));
        }
        if self
            .list_indexes()?
            .iter()
            .any(|index| index.collection.eq_ignore_ascii_case(collection))
            || self
                .list_vector_indexes()?
                .iter()
                .any(|index| index.collection.eq_ignore_ascii_case(collection))
        {
            return Err(CassieError::Unsupported(
                "fresh document load does not maintain secondary indexes".to_string(),
            ));
        }

        let schema = self
            .collection_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let row_schema = self.row_schema(collection)?;
        let write_gate = self.collection_write_gate(collection);
        let _write_guard = write_gate.lock();
        let mut tx = self.begin_data_rw_tx()?;
        let mut ids = Vec::with_capacity(documents.len());

        for (id, payload) in documents {
            Self::validate_document(&schema, &payload)?;
            let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let row_blob = encode_row(&row_schema, &payload)?;
            tx.put(Self::row_key(collection, &id), row_blob, None)
                .map_err(CassieError::from)?;
            Self::write_document_hash_to_tx(&mut tx, collection, &id, &row_schema, &payload)?;
            ids.push(id);
        }

        let row_delta = i64::try_from(ids.len()).unwrap_or(i64::MAX);
        let generation = Self::increment_collection_generation_in_tx(&mut tx, collection)?;
        Self::record_column_batch_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::record_projection_hash_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::increment_data_epoch_in_tx(&mut tx)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        let _ = self.complete_column_batch_maintenance(collection, generation);
        let _ = self.complete_projection_hash_maintenance(collection, generation, row_delta);
        Ok(ids)
    }
}
