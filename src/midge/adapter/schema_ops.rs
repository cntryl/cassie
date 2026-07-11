use super::{
    check_collection_drop_failure_point, check_collection_rename_failure_point,
    check_field_drop_failure_point, check_field_rename_failure_point, key_encoding, CassieError,
    CollectionMeta, FieldConstraint, FieldSchema, IndexKind, IndexMeta, Midge, NamespaceMeta,
    NormalizedVectorRecord, ProjectionMeta, Query, RetentionPolicyMeta, RowSchema, Schema,
    WriteOptions,
};

#[path = "schema_ops_helpers.rs"]
mod schema_ops_helpers;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PendingCollectionRename {
    current_name: String,
    next_name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PendingFieldRename {
    collection: String,
    current_name: String,
    next_name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PendingFieldDrop {
    collection: String,
    field: String,
    #[serde(default)]
    column_names: Vec<String>,
    scalar_names: Vec<String>,
    time_series_names: Vec<String>,
    #[serde(default)]
    vector_names: Vec<String>,
}

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
        if let Some(database) = crate::catalog::schema_database_name(namespace) {
            let _ = self.database_family(&database)?;
        }
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
        let canonical = self.canonical_namespace_name(namespace);
        if !namespaces
            .iter()
            .any(|entry| self.canonical_namespace_name(entry) == canonical)
        {
            namespaces.push(namespace.to_string());
            namespaces.sort();
            Self::save_namespaces(&mut tx, &namespaces)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_namespaces(&self) -> Vec<String> {
        self.list_namespaces_raw()
    }

    pub(crate) fn list_namespaces_canonical(&self) -> Vec<String> {
        self.list_namespaces_raw()
            .into_iter()
            .map(|namespace| self.canonical_namespace_name(&namespace))
            .collect()
    }

    fn list_namespaces_raw(&self) -> Vec<String> {
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
        let mut namespaces = Self::load_namespaces(&tx)?;
        let canonical = self.canonical_namespace_name(namespace);
        let stored = namespaces
            .iter()
            .find(|entry| self.canonical_namespace_name(entry) == canonical)
            .cloned()
            .or_else(|| {
                tx.get(&Self::namespace_key(namespace))
                    .ok()
                    .flatten()
                    .map(|_| namespace.to_string())
            });
        let Some(stored) = stored else {
            return Err(CassieError::NotFound(format!(
                "namespace '{namespace}' does not exist"
            )));
        };

        tx.delete(Self::namespace_key(&stored))
            .map_err(CassieError::from)?;
        namespaces.retain(|entry| entry != &stored);
        Self::save_namespaces(&mut tx, &namespaces)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rename_namespace(&self, current_name: &str, next_name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let mut namespaces = Self::load_namespaces(&tx)?;
        let current_canonical = self.canonical_namespace_name(current_name);
        let current_name = namespaces
            .iter()
            .find(|entry| self.canonical_namespace_name(entry) == current_canonical)
            .cloned()
            .unwrap_or_else(|| current_name.to_string());
        let current_key = Self::namespace_key(&current_name);
        let next_key = Self::namespace_key(next_name);

        let current_raw = tx
            .get(&current_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::NotFound(format!("namespace '{current_name}' does not exist"))
            })?;

        let next_canonical = self.canonical_namespace_name(next_name);
        if tx.get(&next_key).map_err(CassieError::from)?.is_some()
            || namespaces
                .iter()
                .any(|entry| self.canonical_namespace_name(entry) == next_canonical)
        {
            return Err(CassieError::Unsupported(format!(
                "namespace '{next_name}' already exists"
            )));
        }

        let metadata: NamespaceMeta = serde_json::from_slice(&current_raw)
            .map_err(|error| CassieError::Parse(format!("invalid namespace metadata: {error}")))?;
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

        namespaces.retain(|entry| entry != &current_name);
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
        let name_storage = self.canonical_collection_name(name);
        let name = name_storage.as_str();
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
        check_collection_drop_failure_point()?;

        self.delete_collection_data(name)?;

        Ok(())
    }

    pub(super) fn delete_collection_data(&self, name: &str) -> Result<(), CassieError> {
        let mut data_tx = self.begin_data_rw_tx_for(name)?;
        let mut document_keys = Vec::new();
        for data_prefix in [
            Self::row_prefix(name),
            Self::doc_prefix(name),
            Self::scalar_index_collection_prefix(name),
            Self::time_series_index_collection_prefix(name),
            Self::normalized_vector_collection_prefix(name),
            Self::vector_index_state_prefix(name),
            super::key_encoding::unique_constraint_reservation_prefix(name),
            super::key_encoding::unique_index_reservation_prefix(name),
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
        let debt_entries = data_tx
            .scan(&Query::new().prefix(Self::maintenance_debt_prefix().into()))
            .map_err(CassieError::from)?;
        for (key, value) in debt_entries {
            let collection_matches = serde_json::from_slice::<serde_json::Value>(&value)
                .ok()
                .and_then(|debt| {
                    debt.get("collection")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .is_some_and(|collection| collection == name);
            if collection_matches {
                data_tx.delete(key).map_err(CassieError::from)?;
            }
        }
        data_tx
            .delete(Self::collection_generation_key(name))
            .map_err(CassieError::from)?;
        data_tx
            .delete(Self::root_hash_key(name))
            .map_err(CassieError::from)?;
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn alter_collection_add_column(
        &self,
        collection: &str,
        field: FieldSchema,
    ) -> Result<(), CassieError> {
        let collection_storage = self.canonical_collection_name(collection);
        let collection = collection_storage.as_str();
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

        let mut data_tx = self.begin_data_rw_tx_for(collection)?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut data_tx,
            Self::normalized_vector_prefix(collection, &field_name),
        )?;
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        let generation = self.collection_generation(collection)?;
        let mut maintenance_tx = self.begin_data_rw_tx_for(collection)?;
        Self::record_column_batch_maintenance_debt_in_tx(
            &mut maintenance_tx,
            collection,
            generation,
        )?;
        Self::record_projection_hash_maintenance_debt_in_tx(
            &mut maintenance_tx,
            collection,
            generation,
        )?;
        maintenance_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        self.complete_column_batch_maintenance(collection, generation)?;
        self.complete_projection_hash_maintenance(collection, generation, 0)?;
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
        let collection_storage = self.canonical_collection_name(collection);
        let collection = collection_storage.as_str();
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
        let pending = PendingFieldDrop {
            collection: collection.to_string(),
            field: field.to_string(),
            column_names: dropped_indexes.columns.clone(),
            scalar_names: dropped_indexes.scalars.clone(),
            time_series_names: dropped_indexes.time_series.clone(),
            vector_names: dropped_indexes.vectors.clone(),
        };
        tx.put(
            Self::field_drop_operation_key(collection, field),
            serde_json::to_vec(&pending).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        check_field_drop_failure_point()?;
        self.complete_field_drop_data(&pending)?;
        self.clear_pending_field_drop(collection, field)
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
        let collection_storage = self.canonical_collection_name(collection);
        let collection = collection_storage.as_str();
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

        let pending = PendingFieldRename {
            collection: collection.to_string(),
            current_name: current_name.to_string(),
            next_name: next_name.to_string(),
        };
        tx.put(
            Self::field_rename_operation_key(collection, current_name, next_name),
            serde_json::to_vec(&pending).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        check_field_rename_failure_point()?;
        self.complete_field_rename_data(collection, current_name, next_name)?;
        self.clear_pending_field_rename(collection, current_name, next_name)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rename_collection(
        &self,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let current_name_storage = self.canonical_collection_name(current_name);
        let next_name_storage = self.canonical_collection_name(next_name);
        let current_name = current_name_storage.as_str();
        let next_name = next_name_storage.as_str();
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
        schema_ops_helpers::rename_collection_indexes(&mut schema_tx, current_name, next_name)?;
        schema_ops_helpers::rename_collection_retention_policies(
            &mut schema_tx,
            current_name,
            next_name,
        )?;

        schema_ops_helpers::transfer_collection_sidecars(&mut schema_tx, current_name, next_name)?;

        let pending = PendingCollectionRename {
            current_name: current_name.to_string(),
            next_name: next_name.to_string(),
        };
        schema_tx
            .put(
                Self::schema_operation_key(current_name, next_name),
                serde_json::to_vec(&pending)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;

        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        check_collection_rename_failure_point()?;
        self.complete_collection_rename_data(current_name, next_name)?;
        self.clear_pending_collection_rename(current_name, next_name)
    }

    fn complete_collection_rename_data(
        &self,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let mut data_tx = self.begin_data_rw_tx_for(current_name)?;
        schema_ops_helpers::rename_collection_column_batch_metadata(
            &mut data_tx,
            current_name,
            next_name,
        )?;
        schema_ops_helpers::rename_collection_prefixed_data(&mut data_tx, current_name, next_name)?;
        Self::rename_collection_maintenance_debt_in_tx(&mut data_tx, current_name, next_name)?;
        if let Some(generation) = data_tx
            .get(&Self::collection_generation_key(current_name))
            .map_err(CassieError::from)?
        {
            data_tx
                .delete(Self::collection_generation_key(current_name))
                .map_err(CassieError::from)?;
            data_tx
                .put(
                    Self::collection_generation_key(next_name),
                    generation.to_vec(),
                    None,
                )
                .map_err(CassieError::from)?;
        }
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

    fn rename_collection_maintenance_debt_in_tx(
        tx: &mut cntryl_midge::Transaction,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let entries = tx
            .scan(&Query::new().prefix(Self::maintenance_debt_prefix().into()))
            .map_err(CassieError::from)?;
        for (key, value) in entries {
            let Ok(mut debt) =
                serde_json::from_slice::<super::maintenance::MaintenanceDebt>(&value)
            else {
                continue;
            };
            if debt.collection != current_name {
                continue;
            }
            debt.collection = next_name.to_string();
            tx.delete(key).map_err(CassieError::from)?;
            tx.put(
                Self::maintenance_debt_key(next_name, &debt.artifact),
                serde_json::to_vec(&debt).map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        }
        Ok(())
    }

    /// Replays schema operations whose schema commit completed before their data-family work.
    ///
    /// # Errors
    ///
    /// Returns an error when a journal record cannot be read, replayed, or cleared.
    pub fn replay_pending_schema_operations(&self) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let entries = tx
            .scan(&Query::new().prefix(Self::schema_operation_prefix().into()))
            .map_err(CassieError::from)?;
        let pending = entries
            .into_iter()
            .map(|(_, raw)| {
                serde_json::from_slice::<PendingCollectionRename>(&raw).map_err(|error| {
                    CassieError::Parse(format!("invalid schema operation: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for rename in pending {
            if self.collection_schema(&rename.next_name).is_some() {
                self.complete_collection_rename_data(&rename.current_name, &rename.next_name)?;
            }
            self.clear_pending_collection_rename(&rename.current_name, &rename.next_name)?;
        }
        self.replay_pending_field_renames()?;
        self.replay_pending_field_drops()?;
        Ok(())
    }

    fn complete_field_rename_data(
        &self,
        collection: &str,
        current: &str,
        next: &str,
    ) -> Result<(), CassieError> {
        schema_ops_helpers::rename_normalized_vector_records(self, collection, current, next)?;
        self.rebuild_scalar_indexes_for_collection(collection)?;
        self.rebuild_time_series_indexes_for_collection(collection)?;
        let _ = self.rebuild_column_batches_for_collection(collection)?;
        self.rebuild_projection_hashes(collection)?;
        Ok(())
    }

    fn replay_pending_field_renames(&self) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let entries = tx
            .scan(&Query::new().prefix(Self::field_rename_operation_prefix().into()))
            .map_err(CassieError::from)?;
        let pending = entries
            .into_iter()
            .map(|(_, raw)| {
                serde_json::from_slice::<PendingFieldRename>(&raw).map_err(|error| {
                    CassieError::Parse(format!("invalid field rename operation: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for rename in pending {
            let schema_rename_committed =
                self.collection_schema(&rename.collection)
                    .is_some_and(|schema| {
                        schema
                            .fields
                            .iter()
                            .any(|field| field.name.eq_ignore_ascii_case(&rename.next_name))
                            && !schema
                                .fields
                                .iter()
                                .any(|field| field.name.eq_ignore_ascii_case(&rename.current_name))
                    });
            if schema_rename_committed {
                self.complete_field_rename_data(
                    &rename.collection,
                    &rename.current_name,
                    &rename.next_name,
                )?;
            }
            self.clear_pending_field_rename(
                &rename.collection,
                &rename.current_name,
                &rename.next_name,
            )?;
        }
        Ok(())
    }

    fn clear_pending_field_rename(
        &self,
        collection: &str,
        current: &str,
        next: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::field_rename_operation_key(collection, current, next))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    fn complete_field_drop_data(&self, pending: &PendingFieldDrop) -> Result<(), CassieError> {
        schema_ops_helpers::delete_dropped_field_data(
            self,
            &pending.collection,
            &pending.field,
            &schema_ops_helpers::DroppedCollectionIndexes {
                columns: pending.column_names.clone(),
                scalars: pending.scalar_names.clone(),
                time_series: pending.time_series_names.clone(),
                vectors: pending.vector_names.clone(),
            },
        )?;
        self.rebuild_scalar_indexes_for_collection(&pending.collection)?;
        self.rebuild_time_series_indexes_for_collection(&pending.collection)?;
        let _ = self.rebuild_column_batches_for_collection(&pending.collection)?;
        self.rebuild_projection_hashes(&pending.collection)?;
        Ok(())
    }

    fn replay_pending_field_drops(&self) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let entries = tx
            .scan(&Query::new().prefix(Self::field_drop_operation_prefix().into()))
            .map_err(CassieError::from)?;
        let pending = entries
            .into_iter()
            .map(|(_, raw)| {
                serde_json::from_slice::<PendingFieldDrop>(&raw).map_err(|error| {
                    CassieError::Parse(format!("invalid field drop operation: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for drop in pending {
            let committed = self
                .collection_schema(&drop.collection)
                .is_some_and(|schema| {
                    !schema
                        .fields
                        .iter()
                        .any(|entry| entry.name.eq_ignore_ascii_case(&drop.field))
                });
            if committed {
                self.complete_field_drop_data(&drop)?;
            }
            self.clear_pending_field_drop(&drop.collection, &drop.field)?;
        }
        Ok(())
    }

    fn clear_pending_field_drop(&self, collection: &str, field: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::field_drop_operation_key(collection, field))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    fn clear_pending_collection_rename(
        &self,
        current: &str,
        next: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::schema_operation_key(current, next))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }
}
