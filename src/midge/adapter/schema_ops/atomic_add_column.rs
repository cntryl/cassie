use super::super::transactions::{ExpectedStorageValue, StorageMutation};
use super::super::{
    key_encoding, CassieError, DocumentWriteBatchOptions, DocumentWriteOp, FieldConstraint,
    FieldSchema, Midge, ProjectionMeta, RowSchema, Schema, StorageOverlay,
};
use super::PendingFieldAdd;

impl StorageOverlay {
    pub(crate) fn stage_untracked(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.changes.insert(
            key,
            StorageMutation {
                value: Some(value),
                ttl_seconds: None,
                expected: ExpectedStorageValue::Untracked,
            },
        );
    }
}

impl Midge {
    pub(crate) fn legacy_field_add_failure_injected(&self) -> bool {
        super::super::FIELD_ADD_FAILPOINT.get() || super::super::FIELD_ADD_BACKFILL_FAILPOINT.get()
    }

    pub(crate) fn alter_collection_add_column_atomically(
        &self,
        collection: &str,
        field: FieldSchema,
        default_value: Option<serde_json::Value>,
        constraints: &[FieldConstraint],
        sequences: &[crate::catalog::SequenceMeta],
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let database = crate::catalog::relation_database_name(&collection)
            .unwrap_or_else(|| self.default_database.clone());
        let read_tx = self.begin_data_readonly_tx_for(&collection)?;
        let schema_key = Self::collection_schema_key(&collection);
        let schema_raw = read_tx
            .get(&schema_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.clone()))?;
        let mut schema: Schema = serde_json::from_slice(&schema_raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        if schema.fields.iter().any(|entry| entry.name == field.name) {
            return Err(CassieError::Unsupported(format!(
                "field '{0}' already exists on collection '{collection}'",
                field.name
            )));
        }
        let mut row_schema = Self::load_row_schema_from_tx(&read_tx, &collection)?
            .unwrap_or_else(|| RowSchema::from_schema(&schema));
        row_schema.add_field(field.clone())?;
        schema.fields.push(field);

        let mut overlay = StorageOverlay::default();
        overlay.stage_untracked(
            schema_key,
            serde_json::to_vec(&schema).map_err(|error| CassieError::Parse(error.to_string()))?,
        );
        overlay.stage_untracked(
            Self::row_schema_key(&collection),
            serde_json::to_vec(&row_schema)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
        );
        let mut projection = Self::load_projection_metadata_from_tx(&read_tx, &collection)?
            .unwrap_or_else(|| ProjectionMeta::new(&collection, row_schema.schema_version));
        projection.schema_version = row_schema.schema_version;
        overlay.stage_untracked(
            Self::projection_key(&collection),
            serde_json::to_vec(&projection)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
        );
        self.stage_add_column_constraints_and_sequences(
            &mut overlay,
            &collection,
            constraints,
            sequences,
        )?;

        let field_name = schema
            .fields
            .last()
            .expect("new field was appended")
            .name
            .clone();
        let operations =
            self.prepare_add_column_backfill_operations(&collection, &field_name, default_value)?;
        let (writes, options, target_generation) =
            self.prepare_add_column_write_batch(&collection, database, operations, &mut overlay)?;
        self.apply_document_write_batches_with_transactional_ddl_and_schema(
            &writes,
            &options,
            &overlay,
            &schema,
            &row_schema,
            constraints,
        )?;
        if let Some(target_generation) = target_generation {
            let _ = self.complete_column_batch_maintenance(&collection, target_generation, None);
            let _ = self.complete_projection_hash_maintenance(&collection, target_generation, 0);
        }
        Ok(())
    }

    fn prepare_add_column_write_batch(
        &self,
        collection: &str,
        database: String,
        operations: Vec<DocumentWriteOp>,
        overlay: &mut StorageOverlay,
    ) -> Result<
        (
            std::collections::BTreeMap<String, Vec<DocumentWriteOp>>,
            DocumentWriteBatchOptions,
            Option<u64>,
        ),
        CassieError,
    > {
        let no_document_changes = operations.is_empty();
        let target_generation = if no_document_changes {
            let target_generation = self.collection_generation(collection)?.saturating_add(1);
            overlay.stage_untracked(
                Self::collection_generation_key(collection),
                target_generation.to_be_bytes().to_vec(),
            );
            Some(target_generation)
        } else {
            None
        };
        let mut writes = std::collections::BTreeMap::new();
        writes.insert(collection.to_string(), operations);
        let mut options = DocumentWriteBatchOptions::sync(self.write_options_sync());
        options.data_database = Some(database);
        options.normalized_vector_collection = Some(collection.to_string());
        Self::stage_add_column_maintenance_debts(&mut options, collection, target_generation)?;
        Ok((writes, options, target_generation))
    }

    fn stage_add_column_maintenance_debts(
        options: &mut DocumentWriteBatchOptions,
        collection: &str,
        target_generation: Option<u64>,
    ) -> Result<(), CassieError> {
        if let Some(target_generation) = target_generation {
            let column_batch_debt =
                Self::column_batch_maintenance_debt_write(collection, target_generation)?;
            let projection_hash_debt =
                Self::projection_hash_maintenance_debt_write(collection, target_generation)?;
            options
                .additional_data_writes
                .push((column_batch_debt.0, Some(column_batch_debt.1)));
            options
                .additional_data_writes
                .push((projection_hash_debt.0, Some(projection_hash_debt.1)));
        }
        Ok(())
    }

    fn stage_add_column_constraints_and_sequences(
        &self,
        overlay: &mut StorageOverlay,
        collection: &str,
        constraints: &[FieldConstraint],
        sequences: &[crate::catalog::SequenceMeta],
    ) -> Result<(), CassieError> {
        self.require_constraint_definitions(constraints)?;
        overlay.stage_untracked(
            Self::constraints_key(collection),
            serde_json::to_vec(constraints)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
        );
        for sequence in sequences {
            let mut sequence = sequence.clone();
            sequence.name = self.canonical_collection_name(&sequence.name);
            overlay.stage_untracked(
                key_encoding::sequence_key(&sequence.name),
                serde_json::to_vec(&sequence)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
            );
        }
        Ok(())
    }

    fn prepare_add_column_backfill_operations(
        &self,
        collection: &str,
        field_name: &str,
        default_value: Option<serde_json::Value>,
    ) -> Result<Vec<DocumentWriteOp>, CassieError> {
        let mut operations = Vec::new();
        if let Some(default_value) = default_value.filter(|value| !value.is_null()) {
            for batch in self.scan_documents_batched(collection, 256)? {
                for document in batch {
                    let mut payload = document.payload;
                    let object = payload.as_object_mut().ok_or_else(|| {
                        CassieError::InvalidVector("document must be object".to_string())
                    })?;
                    if object.contains_key(field_name) {
                        continue;
                    }
                    object.insert(field_name.to_string(), default_value.clone());
                    operations.push(DocumentWriteOp::Put {
                        id: document.id,
                        payload,
                    });
                }
            }
        }
        Ok(operations)
    }

    pub(crate) fn resume_pending_field_add_matching(
        &self,
        collection: &str,
        field: &FieldSchema,
        default_value: Option<&serde_json::Value>,
    ) -> Result<bool, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::field_add_operation_key(&collection, &field.name))
            .map_err(CassieError::from)?
        else {
            return Ok(false);
        };
        drop(tx);
        let pending = serde_json::from_slice::<PendingFieldAdd>(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid field add operation: {error}")))?;
        if pending.field.name != field.name
            || pending.field.data_type != field.data_type
            || pending.field.nullable != field.nullable
            || pending.default_value.as_ref() != default_value
        {
            return Err(CassieError::Execution(format!(
                "ADD COLUMN retry for '{}' conflicts with the recorded pending definition",
                field.name
            )));
        }
        self.complete_field_add_data(&pending)?;
        self.clear_pending_field_add(&collection, &field.name)?;
        Ok(true)
    }

    pub(crate) fn has_pending_field_add(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<bool, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        tx.get(&Self::field_add_operation_key(&collection, field))
            .map(|value| value.is_some())
            .map_err(CassieError::from)
    }
}
