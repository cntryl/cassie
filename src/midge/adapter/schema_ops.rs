use super::{
    key_encoding, CassieError, CollectionMeta, ColumnBatchMetadata, FieldConstraint, FieldSchema,
    IndexKind, IndexMeta, Midge, NamespaceMeta, NormalizedVectorRecord, ProjectionMeta, Query,
    RetentionPolicyMeta, RowSchema, Schema, WriteOptions,
};

#[path = "schema_ops_helpers.rs"]
mod schema_ops_helpers;

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn create_collection(&self, name: &str, schema: Schema) -> Result<(), CassieError> {
        let result =
            self.create_collection_with_meta(name, &schema, &CollectionMeta::new(name, None));
        let Schema { fields } = schema;
        let _ = fields;
        result
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn create_namespace(&self, namespace: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;

        let namespace_key = Self::namespace_key(namespace);
        if tx.get(&namespace_key).map_err(CassieError::from)?.is_none() {
            let metadata = NamespaceMeta::new(namespace, None);
            let serialized = serde_json::to_vec(&metadata)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(namespace_key, serialized, None)
                .map_err(CassieError::from)?;
        }

        let mut namespaces = Self::load_namespaces(&tx)?;
        if !namespaces.iter().any(|entry| entry == namespace) {
            namespaces.push(namespace.to_string());
            namespaces.sort();
            Self::save_namespaces(&mut tx, &namespaces)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_namespaces(&self) -> Vec<String> {
        let Ok(tx) = self.begin_schema_readonly_tx() else {
            return Vec::new();
        };

        if let Ok(namespaces) = Self::load_namespaces(&tx) {
            if !namespaces.is_empty() {
                let mut namespaces = namespaces;
                namespaces.sort();
                namespaces.dedup();
                return namespaces;
            }
        }

        let Ok(scan) = tx.scan(&Query::new().prefix(Self::namespace_prefix().into())) else {
            return Vec::new();
        };

        let mut namespaces = Vec::new();
        let namespace_prefix = Self::namespace_prefix();
        for (raw_key, _raw_value) in scan {
            if let Some(name) = key_encoding::utf8_suffix_after_prefix(&raw_key, &namespace_prefix)
            {
                namespaces.push(name);
            }
        }

        namespaces.sort();
        namespaces.dedup();
        namespaces
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn drop_namespace(&self, namespace: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let namespace_key = Self::namespace_key(namespace);

        let mut namespaces = Self::load_namespaces(&tx)?;
        let namespace_exists = tx.get(&namespace_key).map_err(CassieError::from)?.is_some()
            || namespaces.iter().any(|entry| entry == namespace);
        if !namespace_exists {
            return Err(CassieError::NotFound(format!(
                "namespace '{namespace}' does not exist"
            )));
        }

        tx.delete(namespace_key).map_err(CassieError::from)?;
        namespaces.retain(|entry| entry != namespace);
        Self::save_namespaces(&mut tx, &namespaces)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rename_namespace(&self, current_name: &str, next_name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let current_key = Self::namespace_key(current_name);
        let next_key = Self::namespace_key(next_name);

        let current_raw = tx
            .get(&current_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::NotFound(format!("namespace '{current_name}' does not exist"))
            })?;

        if tx.get(&next_key).map_err(CassieError::from)?.is_some() {
            return Err(CassieError::Unsupported(format!(
                "namespace '{next_name}' already exists"
            )));
        }

        let metadata: NamespaceMeta = serde_json::from_slice(&current_raw)
            .map_err(|error| CassieError::Parse(format!("invalid namespace metadata: {error}")))?;
        let mut namespaces = Self::load_namespaces(&tx)?;
        if namespaces.is_empty() {
            let scan = tx
                .scan(&Query::new().prefix(Self::namespace_prefix().into()))
                .map_err(CassieError::from)?;
            let namespace_prefix = Self::namespace_prefix();
            for (raw_key, _raw_value) in scan {
                if let Some(name) =
                    key_encoding::utf8_suffix_after_prefix(&raw_key, &namespace_prefix)
                {
                    namespaces.push(name);
                }
            }
        }

        namespaces.retain(|entry| entry != current_name);
        namespaces.push(next_name.to_string());
        namespaces.sort();
        namespaces.dedup();

        tx.delete(current_key).map_err(CassieError::from)?;
        let mut renamed = metadata;
        renamed.name = next_name.to_string();
        tx.put(
            next_key,
            serde_json::to_vec(&renamed).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        Self::save_namespaces(&mut tx, &namespaces)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn drop_collection(&self, name: &str) -> Result<(), CassieError> {
        let mut schema_tx = self.begin_schema_rw_tx()?;
        let schema_key = Self::collection_schema_key(name);
        if schema_tx
            .get(&schema_key)
            .map_err(CassieError::from)?
            .is_none()
        {
            return Err(CassieError::CollectionNotFound(name.to_string()));
        }

        let vector_prefix = Self::vector_index_collection_prefix(name);
        let vector_indexes = schema_tx
            .scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?;
        let mut vector_keys = Vec::new();
        for (key, _value) in vector_indexes {
            vector_keys.push(key);
        }
        for key in vector_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }

        let index_prefix = Self::index_collection_prefix(name);
        let index_scan = schema_tx
            .scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?;
        let mut index_keys = Vec::new();
        for (key, _) in index_scan {
            index_keys.push(key);
        }
        for key in index_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }
        let retention_scan = schema_tx
            .scan(&Query::new().prefix(Self::retention_prefix().into()))
            .map_err(CassieError::from)?;
        let mut retention_keys = Vec::new();
        for (key, value) in retention_scan {
            let Ok(policy) = serde_json::from_slice::<RetentionPolicyMeta>(&value) else {
                continue;
            };
            if policy.collection == name {
                retention_keys.push(key);
            }
        }
        for key in retention_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }
        Self::delete_keys_with_prefix(&mut schema_tx, Self::column_batch_collection_prefix(name))?;
        Self::delete_collection_metadata_to_tx(&mut schema_tx, name)?;

        schema_tx
            .delete(Self::constraints_key(name))
            .map_err(CassieError::from)?;
        schema_tx
            .delete(Self::row_schema_key(name))
            .map_err(CassieError::from)?;
        schema_tx
            .delete(Self::projection_key(name))
            .map_err(CassieError::from)?;
        schema_tx
            .delete(Self::cardinality_key(name))
            .map_err(CassieError::from)?;

        let mut collections = Self::load_collections(&schema_tx)?;
        collections.retain(|entry| entry != name);
        Self::save_collections(&mut schema_tx, &collections)?;
        schema_tx.delete(schema_key).map_err(CassieError::from)?;
        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        let mut document_keys = Vec::new();
        for data_prefix in [
            Self::row_prefix(name),
            Self::doc_prefix(name),
            Self::scalar_index_collection_prefix(name),
            Self::time_series_index_collection_prefix(name),
            Self::normalized_vector_collection_prefix(name),
            Self::column_batch_collection_prefix(name),
            Self::column_store_collection_prefix(name),
            Self::row_hash_prefix(name),
            Self::range_hash_prefix(name),
        ] {
            let documents = data_tx
                .scan(&Query::new().prefix(data_prefix.into()))
                .map_err(CassieError::from)?;
            for (key, _value) in documents {
                document_keys.push(key);
            }
        }

        for key in document_keys {
            data_tx.delete(key).map_err(CassieError::from)?;
        }
        data_tx
            .delete(Self::root_hash_key(name))
            .map_err(CassieError::from)?;
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn alter_collection_add_column(
        &self,
        collection: &str,
        field: FieldSchema,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let schema_key = Self::collection_schema_key(collection);
        let schema_raw = tx.get(&schema_key).map_err(CassieError::from)?;
        let Some(schema_raw) = schema_raw else {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        };

        let mut schema: Schema = serde_json::from_slice(&schema_raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;

        if schema.fields.iter().any(|entry| entry.name == field.name) {
            return Err(CassieError::Unsupported(format!(
                "field '{0}' already exists on collection '{collection}'",
                field.name
            )));
        }

        let mut row_schema = Self::load_row_schema_from_tx(&tx, collection)?
            .unwrap_or_else(|| RowSchema::from_schema(&schema));
        row_schema.add_field(field.clone())?;
        Self::save_row_schema_to_tx(&mut tx, collection, &row_schema)?;
        Self::update_projection_schema_version_to_tx(
            &mut tx,
            collection,
            row_schema.schema_version,
        )?;

        let field_name = field.name.clone();
        schema.fields.push(field);
        let schema_bytes =
            serde_json::to_vec(&schema).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(schema_key, schema_bytes, None)
            .map_err(CassieError::from)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut data_tx,
            Self::normalized_vector_prefix(collection, &field_name),
        )?;
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        let _ = self.rebuild_column_batches_for_collection(collection)?;
        self.rebuild_projection_hashes(collection)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn alter_collection_drop_column(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let schema_key = Self::collection_schema_key(collection);
        let schema_raw = tx.get(&schema_key).map_err(CassieError::from)?;
        let Some(schema_raw) = schema_raw else {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        };

        let mut schema: Schema = serde_json::from_slice(&schema_raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        let original_schema = schema.clone();

        let field_count_before = schema.fields.len();
        schema.fields.retain(|entry| entry.name != field);
        if schema.fields.len() == field_count_before {
            return Err(CassieError::Unsupported(format!(
                "field '{field}' not found in collection '{collection}'",
            )));
        }

        let mut row_schema = Self::load_row_schema_from_tx(&tx, collection)?
            .unwrap_or_else(|| RowSchema::from_schema(&original_schema));
        if !row_schema.retire_field(field) {
            return Err(CassieError::Unsupported(format!(
                "field '{field}' not found in collection '{collection}'",
            )));
        }
        Self::save_row_schema_to_tx(&mut tx, collection, &row_schema)?;
        Self::update_projection_schema_version_to_tx(
            &mut tx,
            collection,
            row_schema.schema_version,
        )?;

        let schema_bytes =
            serde_json::to_vec(&schema).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(schema_key, schema_bytes, None)
            .map_err(CassieError::from)?;
        let dropped_indexes =
            schema_ops_helpers::drop_referencing_indexes_in_tx(&mut tx, collection, field)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        schema_ops_helpers::delete_dropped_field_data(self, collection, field, dropped_indexes)?;
        self.rebuild_scalar_indexes_for_collection(collection)?;
        self.rebuild_time_series_indexes_for_collection(collection)?;
        let _ = self.rebuild_column_batches_for_collection(collection)?;
        self.rebuild_projection_hashes(collection)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn alter_collection_rename_column(
        &self,
        collection: &str,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let schema_key = Self::collection_schema_key(collection);
        let schema_raw = tx.get(&schema_key).map_err(CassieError::from)?;
        let Some(schema_raw) = schema_raw else {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        };

        let mut schema: Schema = serde_json::from_slice(&schema_raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        let original_schema = schema.clone();

        if schema
            .fields
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(next_name))
        {
            return Err(CassieError::Unsupported(format!(
                "field '{next_name}' already exists on collection '{collection}'"
            )));
        }

        let Some(field) = schema
            .fields
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(current_name))
        else {
            return Err(CassieError::Unsupported(format!(
                "field '{current_name}' not found in collection '{collection}'"
            )));
        };
        field.name = next_name.to_string();

        let mut row_schema = Self::load_row_schema_from_tx(&tx, collection)?
            .unwrap_or_else(|| RowSchema::from_schema(&original_schema));
        row_schema.rename_field(current_name, next_name)?;
        Self::save_row_schema_to_tx(&mut tx, collection, &row_schema)?;
        Self::update_projection_schema_version_to_tx(
            &mut tx,
            collection,
            row_schema.schema_version,
        )?;

        let schema_bytes =
            serde_json::to_vec(&schema).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(schema_key, schema_bytes, None)
            .map_err(CassieError::from)?;
        schema_ops_helpers::rename_constraints_in_tx(&mut tx, collection, current_name, next_name)?;
        schema_ops_helpers::rename_indexes_in_tx(&mut tx, collection, current_name, next_name)?;
        schema_ops_helpers::rename_vector_indexes_in_tx(
            &mut tx,
            collection,
            current_name,
            next_name,
        )?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        schema_ops_helpers::rename_normalized_vector_records(
            self,
            collection,
            current_name,
            next_name,
        )?;
        self.rebuild_scalar_indexes_for_collection(collection)?;
        self.rebuild_time_series_indexes_for_collection(collection)?;
        let _ = self.rebuild_column_batches_for_collection(collection)?;
        self.rebuild_projection_hashes(collection)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rename_collection(
        &self,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let mut schema_tx = self.begin_schema_rw_tx()?;
        schema_ops_helpers::rename_collection_schema_entries(
            &mut schema_tx,
            current_name,
            next_name,
        )?;

        schema_ops_helpers::rename_collection_vector_indexes(
            &mut schema_tx,
            current_name,
            next_name,
        )?;
        schema_ops_helpers::rename_collection_column_batch_metadata(
            &mut schema_tx,
            current_name,
            next_name,
        )?;
        schema_ops_helpers::rename_collection_indexes(&mut schema_tx, current_name, next_name)?;
        schema_ops_helpers::rename_collection_retention_policies(
            &mut schema_tx,
            current_name,
            next_name,
        )?;

        schema_ops_helpers::transfer_collection_sidecars(&mut schema_tx, current_name, next_name)?;

        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        schema_ops_helpers::rename_collection_prefixed_data(&mut data_tx, current_name, next_name)?;
        if let Some(root) = data_tx
            .get(&Self::root_hash_key(current_name))
            .map_err(CassieError::from)?
        {
            data_tx
                .delete(Self::root_hash_key(current_name))
                .map_err(CassieError::from)?;
            data_tx
                .put(Self::root_hash_key(next_name), root.to_vec(), None)
                .map_err(CassieError::from)?;
        }
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        self.rebuild_time_series_indexes_for_collection(next_name)?;
        self.rebuild_projection_hashes(next_name)?;

        Ok(())
    }
}
