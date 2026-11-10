//! Current row encoding descriptors and focused descriptor type updates.

use super::{CassieError, Midge, RowSchema, Schema};

impl Midge {
    /// Delayed work checks published identity, never an enclosing stale view.
    pub(crate) fn current_row_schema_identity(
        &self,
        collection: &str,
    ) -> Result<Option<u64>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let database = crate::catalog::relation_database_name(&collection)
            .unwrap_or_else(|| self.default_database.clone());
        let tx =
            self.begin_raw_database_adapter_tx(&database, super::super::TransactionMode::ReadOnly)?;
        if let Some(schema) = Self::load_row_schema_from_tx(&tx, &collection)? {
            return Ok(Some(schema.relation_id));
        }
        Ok(tx
            .get(&Self::collection_schema_key(&collection))
            .map_err(CassieError::from)?
            .map(|_| 0))
    }

    pub(crate) fn row_schema(&self, collection: &str) -> Result<RowSchema, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let statement_read = self.begin_data_readonly_tx_for(&collection)?;
        // RR keeps current name/encoding descriptors distinct from retained
        // rows. Ordinary RC statement views retain their existing Data metadata.
        let tx =
            if statement_read.uses_current_descriptors() {
                super::super::DataReadTransaction::fresh(self.database_tx_for_collection(
                    &collection,
                    super::super::TransactionMode::ReadOnly,
                )?)
            } else {
                statement_read
            };
        if let Some(row_schema) = Self::load_row_schema_from_tx(&tx, &collection)? {
            return Ok(row_schema);
        }

        let raw = tx
            .get(&Self::collection_schema_key(&collection))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.clone()))?;
        let schema: Schema = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        Ok(RowSchema::from_schema(&schema))
    }

    pub(crate) fn alter_collection_column_type(
        &self,
        collection: &str,
        field: &str,
        data_type: crate::types::DataType,
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        let schema_key = Self::collection_schema_key(&collection);
        let raw = tx
            .get(&schema_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.clone()))?;
        let mut schema: Schema = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        let Some(schema_field) = schema.fields.iter_mut().find(|entry| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(field, &entry.name)
        }) else {
            return Err(CassieError::Unsupported(format!(
                "field '{field}' not found in collection '{collection}'"
            )));
        };
        schema_field.data_type.clone_from(&data_type);

        let mut row_schema = Self::load_row_schema_from_tx(&tx, &collection)?
            .unwrap_or_else(|| RowSchema::from_schema(&schema));
        let Some(row_field) = row_schema.fields.iter_mut().find(|entry| {
            !entry.retired
                && crate::sql::ColumnIdentifierPath::matches_stored_field(field, &entry.name)
        }) else {
            return Err(CassieError::Unsupported(format!(
                "field '{field}' not found in row schema for '{collection}'"
            )));
        };
        row_field.data_type = data_type;
        row_schema.schema_version = row_schema.schema_version.saturating_add(1);
        Self::save_row_schema_to_tx(&mut tx, &collection, &row_schema)?;
        Self::update_projection_schema_version_to_tx(
            &mut tx,
            &collection,
            row_schema.schema_version,
        )?;
        tx.put(
            schema_key,
            serde_json::to_vec(&schema).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }
}
