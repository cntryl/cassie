use super::*;

impl Midge {
    pub fn create_collection(&self, name: &str, schema: Schema) -> Result<(), CassieError> {
        self.create_collection_with_meta(name, schema, CollectionMeta::new(name, None))
    }

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

        let mut namespaces = self.load_namespaces(&tx)?;
        if !namespaces.iter().any(|entry| entry == namespace) {
            namespaces.push(namespace.to_string());
            namespaces.sort();
            self.save_namespaces(&mut tx, &namespaces)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_namespaces(&self) -> Vec<String> {
        let tx = match self.begin_schema_readonly_tx() {
            Ok(tx) => tx,
            Err(_) => return Vec::new(),
        };

        if let Ok(namespaces) = self.load_namespaces(&tx) {
            if !namespaces.is_empty() {
                let mut namespaces = namespaces;
                namespaces.sort();
                namespaces.dedup();
                return namespaces;
            }
        }

        let Ok(mut scan) = tx.scan(&Query::new().prefix(Self::namespace_prefix().into())) else {
            return Vec::new();
        };

        let mut namespaces = Vec::new();
        while let Some((raw_key, _raw_value)) = scan.next() {
            let key = String::from_utf8(raw_key).unwrap_or_default();
            let name = key
                .strip_prefix(SCHEMA_NAMESPACE_KEY_PREFIX)
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                namespaces.push(name);
            }
        }

        namespaces.sort();
        namespaces.dedup();
        namespaces
    }

    pub fn drop_namespace(&self, namespace: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let namespace_key = Self::namespace_key(namespace);

        let mut namespaces = self.load_namespaces(&tx)?;
        let namespace_exists = tx.get(&namespace_key).map_err(CassieError::from)?.is_some()
            || namespaces.iter().any(|entry| entry == namespace);
        if !namespace_exists {
            return Err(CassieError::NotFound(format!(
                "namespace '{namespace}' does not exist"
            )));
        }

        tx.delete(namespace_key).map_err(CassieError::from)?;
        namespaces.retain(|entry| entry != namespace);
        self.save_namespaces(&mut tx, &namespaces)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

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
        let mut namespaces = self.load_namespaces(&tx)?;
        if namespaces.is_empty() {
            let mut scan = tx
                .scan(&Query::new().prefix(Self::namespace_prefix().into()))
                .map_err(CassieError::from)?;
            while let Some((raw_key, _raw_value)) = scan.next() {
                let key = String::from_utf8(raw_key).unwrap_or_default();
                let name = key
                    .strip_prefix(SCHEMA_NAMESPACE_KEY_PREFIX)
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
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
        self.save_namespaces(&mut tx, &namespaces)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

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
        let mut vector_indexes = schema_tx
            .scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?;
        let mut vector_keys = Vec::new();
        while let Some((key, _value)) = vector_indexes.next() {
            vector_keys.push(key);
        }
        for key in vector_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }

        let index_prefix = Self::index_collection_prefix(name);
        let mut index_scan = schema_tx
            .scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?;
        let mut index_keys = Vec::new();
        while let Some((key, _)) = index_scan.next() {
            index_keys.push(key);
        }
        for key in index_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }
        let mut retention_scan = schema_tx
            .scan(&Query::new().prefix(Self::retention_prefix().into()))
            .map_err(CassieError::from)?;
        let mut retention_keys = Vec::new();
        while let Some((key, value)) = retention_scan.next() {
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

        let mut collections = self.load_collections(&schema_tx)?;
        collections.retain(|entry| entry != name);
        self.save_collections(&mut schema_tx, &collections)?;
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
            Self::normalized_vector_collection_prefix(name),
            Self::column_batch_collection_prefix(name),
            Self::column_store_collection_prefix(name),
            Self::row_hash_prefix(name),
            Self::range_hash_prefix(name),
        ] {
            let mut documents = data_tx
                .scan(&Query::new().prefix(data_prefix.into()))
                .map_err(CassieError::from)?;
            while let Some((key, _value)) = documents.next() {
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
        let index_prefix = Self::index_collection_prefix(collection);
        let mut indexes = tx
            .scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?;
        let mut dropped_column_index_keys = Vec::new();
        let mut dropped_scalar_indexes = Vec::new();
        while let Some((key, value)) = indexes.next() {
            let Ok(metadata) = serde_json::from_slice::<IndexMeta>(&value) else {
                continue;
            };
            let references_field = metadata
                .normalized_fields()
                .iter()
                .chain(metadata.normalized_include_fields().iter())
                .any(|candidate| candidate.eq_ignore_ascii_case(field));
            if !references_field {
                continue;
            }
            match metadata.kind {
                IndexKind::Column => {
                    dropped_column_index_keys.push((key, metadata.name));
                }
                IndexKind::Scalar => {
                    dropped_scalar_indexes.push((key, metadata.name));
                }
                _ => {}
            }
        }
        for (key, index_name) in dropped_column_index_keys {
            tx.delete(key).map_err(CassieError::from)?;
            Self::delete_keys_with_prefix(
                &mut tx,
                Self::column_batch_index_prefix(collection, &index_name),
            )?;
        }
        let mut dropped_scalar_index_names = Vec::new();
        for (key, index_name) in dropped_scalar_indexes {
            tx.delete(key).map_err(CassieError::from)?;
            dropped_scalar_index_names.push(index_name);
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut data_tx,
            Self::normalized_vector_prefix(collection, field),
        )?;
        for index_name in dropped_scalar_index_names {
            Self::delete_keys_with_prefix(
                &mut data_tx,
                Self::scalar_index_data_prefix(collection, &index_name),
            )?;
        }
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        self.rebuild_scalar_indexes_for_collection(collection)?;
        let _ = self.rebuild_column_batches_for_collection(collection)?;
        self.rebuild_projection_hashes(collection)?;
        Ok(())
    }

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

        if let Some(raw_constraints) = tx
            .get(&Self::constraints_key(collection))
            .map_err(CassieError::from)?
        {
            let mut constraints: Vec<FieldConstraint> = serde_json::from_slice(&raw_constraints)
                .map_err(|error| {
                    CassieError::Parse(format!(
                        "invalid constraint metadata for '{collection}': {error}"
                    ))
                })?;
            let mut changed = false;
            for constraint in &mut constraints {
                if constraint.field.eq_ignore_ascii_case(current_name) {
                    constraint.field = next_name.to_string();
                    changed = true;
                }
                if let Some(check) = constraint.check.as_mut() {
                    if check.field.eq_ignore_ascii_case(current_name) {
                        check.field = next_name.to_string();
                        changed = true;
                    }
                }
            }
            if changed {
                let value = serde_json::to_vec(&constraints)
                    .map_err(|error| CassieError::Parse(error.to_string()))?;
                tx.put(Self::constraints_key(collection), value, None)
                    .map_err(CassieError::from)?;
            }
        }

        let index_prefix = Self::index_collection_prefix(collection);
        let mut indexes = tx
            .scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?;
        let mut index_keys = Vec::new();
        while let Some((key, _value)) = indexes.next() {
            index_keys.push(key);
        }
        for key in index_keys {
            let Some(raw_value) = tx.get(&key).map_err(CassieError::from)? else {
                continue;
            };
            let Ok(mut metadata) = serde_json::from_slice::<IndexMeta>(&raw_value) else {
                continue;
            };
            if metadata.rename_field(current_name, next_name) {
                let value = serde_json::to_vec(&metadata)
                    .map_err(|error| CassieError::Parse(error.to_string()))?;
                tx.put(key, value, None).map_err(CassieError::from)?;
            }
        }

        let vector_prefix = Self::vector_index_collection_prefix(collection);
        let mut vector_indexes = tx
            .scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?;
        let mut vector_keys = Vec::new();
        while let Some((key, _value)) = vector_indexes.next() {
            vector_keys.push(key);
        }

        for key in vector_keys {
            let Some(raw_value) = tx.get(&key).map_err(CassieError::from)? else {
                continue;
            };
            let Ok(mut record) =
                serde_json::from_slice::<crate::embeddings::VectorIndexRecord>(&raw_value)
            else {
                continue;
            };

            let mut changed = false;
            let mut next_key = key.clone();
            if record.field.eq_ignore_ascii_case(current_name) {
                record.field = next_name.to_string();
                next_key = Self::vector_index_key(&record.collection, &record.field);
                changed = true;
            }
            if record.source_field.eq_ignore_ascii_case(current_name) {
                record.source_field = next_name.to_string();
                changed = true;
            }

            if changed {
                if next_key != key {
                    tx.delete(key).map_err(CassieError::from)?;
                }
                let value = serde_json::to_vec(&record)
                    .map_err(|error| CassieError::Parse(error.to_string()))?;
                tx.put(next_key, value, None).map_err(CassieError::from)?;
            }
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        let mut scan = data_tx
            .scan(
                &Query::new()
                    .prefix(Self::normalized_vector_prefix(collection, current_name).into()),
            )
            .map_err(CassieError::from)?;
        let mut entries = Vec::new();
        while let Some((key, value)) = scan.next() {
            entries.push((key, value));
        }
        for (key, value) in entries {
            let mut record: NormalizedVectorRecord =
                serde_json::from_slice(&value).map_err(|error| {
                    CassieError::Parse(format!(
                    "invalid normalized vector metadata for '{collection}.{current_name}': {error}"
                ))
                })?;
            record.field = next_name.to_string();
            let next_key = Self::normalized_vector_key(collection, next_name, &record.id);
            data_tx.delete(key).map_err(CassieError::from)?;
            data_tx
                .put(
                    next_key,
                    serde_json::to_vec(&record)
                        .map_err(|error| CassieError::Parse(error.to_string()))?,
                    None,
                )
                .map_err(CassieError::from)?;
        }
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        self.rebuild_scalar_indexes_for_collection(collection)?;
        let _ = self.rebuild_column_batches_for_collection(collection)?;
        self.rebuild_projection_hashes(collection)?;
        Ok(())
    }

    pub fn rename_collection(
        &self,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let mut schema_tx = self.begin_schema_rw_tx()?;

        let current_schema_key = Self::collection_schema_key(current_name);
        let current_schema_bytes = schema_tx
            .get(&current_schema_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(current_name.to_string()))?;

        let next_schema_key = Self::collection_schema_key(next_name);
        if schema_tx
            .get(&next_schema_key)
            .map_err(CassieError::from)?
            .is_some()
        {
            return Err(CassieError::Unsupported(format!(
                "collection '{next_name}' already exists"
            )));
        }

        schema_tx
            .delete(current_schema_key)
            .map_err(CassieError::from)?;
        schema_tx
            .put(next_schema_key, current_schema_bytes.to_vec(), None)
            .map_err(CassieError::from)?;

        let current_row_schema_key = Self::row_schema_key(current_name);
        if let Some(row_schema_bytes) = schema_tx
            .get(&current_row_schema_key)
            .map_err(CassieError::from)?
        {
            schema_tx
                .delete(current_row_schema_key)
                .map_err(CassieError::from)?;
            schema_tx
                .put(
                    Self::row_schema_key(next_name),
                    row_schema_bytes.to_vec(),
                    None,
                )
                .map_err(CassieError::from)?;
        }
        Self::rename_collection_metadata_to_tx(&mut schema_tx, current_name, next_name)?;

        let current_projection_key = Self::projection_key(current_name);
        if let Some(projection_bytes) = schema_tx
            .get(&current_projection_key)
            .map_err(CassieError::from)?
        {
            let mut metadata: ProjectionMeta =
                serde_json::from_slice(&projection_bytes).map_err(|error| {
                    CassieError::Parse(format!(
                        "invalid projection metadata for '{current_name}': {error}"
                    ))
                })?;
            if metadata.projection_id == current_name {
                metadata.projection_id = next_name.to_string();
            }
            metadata.collection = next_name.to_string();
            schema_tx
                .delete(current_projection_key)
                .map_err(CassieError::from)?;
            Self::save_projection_metadata_to_tx(&mut schema_tx, &metadata)?;
        }

        let mut collections = self.load_collections(&schema_tx)?;
        if let Some(position) = collections.iter().position(|entry| entry == current_name) {
            collections[position] = next_name.to_string();
            collections.sort();
            collections.dedup();
            self.save_collections(&mut schema_tx, &collections)?;
        }

        let vector_prefix = Self::vector_index_collection_prefix(current_name);
        let mut vector_indexes = schema_tx
            .scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?;
        let mut vector_keys = Vec::new();
        while let Some((key, _value)) = vector_indexes.next() {
            vector_keys.push(key);
        }

        for key in vector_keys {
            let Some(raw_value) = schema_tx.get(&key).map_err(CassieError::from)? else {
                continue;
            };
            let Ok(mut record) =
                serde_json::from_slice::<crate::embeddings::VectorIndexRecord>(&raw_value)
            else {
                continue;
            };

            record.collection = next_name.to_string();
            schema_tx.delete(key).map_err(CassieError::from)?;
            let next_key = Self::vector_index_key(&record.collection, &record.field);
            let value = serde_json::to_vec(&record)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            schema_tx
                .put(next_key, value, None)
                .map_err(CassieError::from)?;
        }

        let index_prefix = Self::index_collection_prefix(current_name);
        let mut indexes = schema_tx
            .scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?;
        let mut index_keys = Vec::new();
        while let Some((key, _value)) = indexes.next() {
            index_keys.push(key);
        }
        for key in index_keys {
            let Some(raw_value) = schema_tx.get(&key).map_err(CassieError::from)? else {
                continue;
            };
            let Ok(mut metadata) = serde_json::from_slice::<IndexMeta>(&raw_value) else {
                continue;
            };

            metadata.collection = next_name.to_string();
            schema_tx.delete(key).map_err(CassieError::from)?;
            let next_key = Self::index_key(&metadata.collection, &metadata.name);
            let value = serde_json::to_vec(&metadata)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            schema_tx
                .put(next_key, value, None)
                .map_err(CassieError::from)?;
        }

        let mut retention_scan = schema_tx
            .scan(&Query::new().prefix(Self::retention_prefix().into()))
            .map_err(CassieError::from)?;
        let mut retention_entries = Vec::new();
        while let Some((key, value)) = retention_scan.next() {
            let Ok(mut policy) = serde_json::from_slice::<RetentionPolicyMeta>(&value) else {
                continue;
            };
            if policy.collection == current_name {
                policy.collection = next_name.to_string();
                retention_entries.push((key, policy));
            }
        }
        for (key, policy) in retention_entries {
            schema_tx.delete(key).map_err(CassieError::from)?;
            let value = serde_json::to_vec(&policy)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            schema_tx
                .put(Self::retention_key(&policy.name), value, None)
                .map_err(CassieError::from)?;
        }

        let current_column_batch_prefix = Self::column_batch_collection_prefix(current_name);
        let next_column_batch_prefix = Self::column_batch_collection_prefix(next_name);
        let mut column_batches = schema_tx
            .scan(&Query::new().prefix(current_column_batch_prefix.clone().into()))
            .map_err(CassieError::from)?;
        let mut column_batch_entries = Vec::new();
        while let Some((key, value)) = column_batches.next() {
            column_batch_entries.push((key, value));
        }
        for (key, value) in column_batch_entries {
            let Some(suffix) = key.strip_prefix(current_column_batch_prefix.as_slice()) else {
                continue;
            };
            let next_key = [next_column_batch_prefix.as_slice(), suffix].concat();
            schema_tx.delete(key.clone()).map_err(CassieError::from)?;
            let mut metadata: ColumnBatchMetadata =
                serde_json::from_slice(&value).map_err(|error| {
                    CassieError::Parse(format!("invalid column batch metadata: {error}"))
                })?;
            metadata.collection = next_name.to_string();
            schema_tx
                .put(
                    next_key,
                    serde_json::to_vec(&metadata)
                        .map_err(|error| CassieError::Parse(error.to_string()))?,
                    None,
                )
                .map_err(CassieError::from)?;
        }

        let current_constraints_key = Self::constraints_key(current_name);
        let constraints = schema_tx
            .get(&current_constraints_key)
            .map_err(CassieError::from)?;
        if let Some(raw) = constraints {
            schema_tx
                .delete(current_constraints_key)
                .map_err(CassieError::from)?;
            schema_tx
                .put(Self::constraints_key(next_name), raw.to_vec(), None)
                .map_err(CassieError::from)?;
        }

        let current_cardinality_key = Self::cardinality_key(current_name);
        if let Some(raw) = schema_tx
            .get(current_cardinality_key.as_slice())
            .map_err(CassieError::from)?
        {
            schema_tx
                .delete(current_cardinality_key)
                .map_err(CassieError::from)?;
            schema_tx
                .put(Self::cardinality_key(next_name), raw.to_vec(), None)
                .map_err(CassieError::from)?;
        }

        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        for (current_prefix, next_prefix) in [
            (Self::row_prefix(current_name), Self::row_prefix(next_name)),
            (Self::doc_prefix(current_name), Self::doc_prefix(next_name)),
            (
                Self::scalar_index_collection_prefix(current_name),
                Self::scalar_index_collection_prefix(next_name),
            ),
            (
                Self::normalized_vector_collection_prefix(current_name),
                Self::normalized_vector_collection_prefix(next_name),
            ),
            (
                Self::column_store_collection_prefix(current_name),
                Self::column_store_collection_prefix(next_name),
            ),
            (
                Self::column_batch_collection_prefix(current_name),
                Self::column_batch_collection_prefix(next_name),
            ),
            (
                Self::row_hash_prefix(current_name),
                Self::row_hash_prefix(next_name),
            ),
            (
                Self::range_hash_prefix(current_name),
                Self::range_hash_prefix(next_name),
            ),
        ] {
            let mut documents = data_tx
                .scan(&Query::new().prefix(current_prefix.clone().into()))
                .map_err(CassieError::from)?;
            let mut entries = Vec::new();
            while let Some((key, value)) = documents.next() {
                entries.push((key, value));
            }

            for (key, value) in entries {
                if let Some(id) = key.strip_prefix(current_prefix.as_slice()) {
                    data_tx.delete(key.clone()).map_err(CassieError::from)?;

                    if current_prefix.starts_with(NORMALIZED_VECTOR_PREFIX.as_bytes()) {
                        let mut record: NormalizedVectorRecord =
                            serde_json::from_slice(&value).map_err(|error| {
                                CassieError::Parse(format!(
                                    "invalid normalized vector metadata for '{current_name}': {error}"
                                ))
                            })?;
                        record.collection = next_name.to_string();
                        let next_key = Self::normalized_vector_key(
                            &record.collection,
                            &record.field,
                            &record.id,
                        );
                        data_tx
                            .put(
                                next_key,
                                serde_json::to_vec(&record)
                                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                                None,
                            )
                            .map_err(CassieError::from)?;
                    } else {
                        let next_key = [next_prefix.as_slice(), id].concat();
                        data_tx
                            .put(next_key, value, None)
                            .map_err(CassieError::from)?;
                    }
                }
            }
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
        self.rebuild_projection_hashes(next_name)?;

        Ok(())
    }
}
