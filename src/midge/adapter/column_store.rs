use super::{
    collect_scan, key_encoding, CassieError, CollectionCardinalityStats, CollectionMeta,
    CollectionStorageMode, DocumentRef, HashSet, Instant, Midge, MidgeScanTimings, OrderedRowBound,
    ProjectionMeta, Query, RowFilter, RowSchema, Schema, WriteOptions,
};
use std::time::Duration;

#[derive(Clone, Copy)]
pub(crate) struct ColumnStoreScanRequest<'a> {
    pub collection: &'a str,
    pub row_schema: &'a RowSchema,
    pub batch_size: usize,
    pub projection: Option<&'a HashSet<String>>,
    pub filter: Option<&'a RowFilter>,
    pub limit: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct OrderedColumnStoreScanRequest<'a> {
    pub collection: &'a str,
    pub row_schema: &'a RowSchema,
    pub batch_size: usize,
    pub projection: Option<&'a HashSet<String>>,
    pub start_bound: Option<&'a OrderedRowBound>,
    pub end_bound: Option<&'a OrderedRowBound>,
    pub reverse: bool,
    pub limit: usize,
}

impl Midge {
    #[doc(hidden)]
    pub fn column_store_prefix_for_diagnostics(
        &self,
        collection: &str,
    ) -> Result<Vec<u8>, CassieError> {
        let relation_id = self
            .collection_metadata(collection)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?
            .storage_id;
        Ok(Self::column_store_collection_prefix(relation_id))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn create_collection_with_meta(
        &self,
        name: &str,
        schema: &Schema,
        metadata: &CollectionMeta,
    ) -> Result<(), CassieError> {
        let name = self.canonical_collection_name(name);
        let mut metadata = metadata.clone();
        metadata.name.clone_from(&name);
        if let Some(database) = crate::catalog::relation_database_name(&name) {
            let _ = self.database_family(&database)?;
        }
        let mut tx = self.begin_schema_rw_tx()?;

        let schema_key = Self::collection_schema_key(&name);
        if tx.get(&schema_key).map_err(CassieError::from)?.is_none() {
            let schema_bytes = serde_json::to_vec(schema)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(schema_key, schema_bytes, None)
                .map_err(CassieError::from)?;
        }
        let existing_metadata = Self::load_collection_metadata_from_tx(&tx, &name)?;
        let storage_id = if let Some(existing) = existing_metadata.as_ref() {
            existing.storage_id
        } else {
            Self::allocate_object_id_to_tx(&mut tx)?
        };
        metadata.storage_id = storage_id;
        let mut row_schema = RowSchema::from_schema(schema);
        row_schema.relation_id = storage_id;
        if tx
            .get(&Self::row_schema_key(&name))
            .map_err(CassieError::from)?
            .is_none()
        {
            Self::save_row_schema_to_tx(&mut tx, &name, &row_schema)?;
        }
        if tx
            .get(&Self::projection_key(&name))
            .map_err(CassieError::from)?
            .is_none()
        {
            Self::save_projection_metadata_to_tx(
                &mut tx,
                &ProjectionMeta::new(&name, row_schema.schema_version),
            )?;
        }
        if tx
            .get(Self::cardinality_key(&name).as_slice())
            .map_err(CassieError::from)?
            .is_none()
        {
            Self::save_cardinality_stats_to_tx(
                &mut tx,
                &name,
                &CollectionCardinalityStats::default(),
            )?;
        }
        if existing_metadata.is_none() {
            Self::save_collection_metadata_to_tx(&mut tx, &metadata)?;
        }

        let mut collections = Self::load_collections(&tx)?;
        if !collections.iter().any(|entry| entry == &name) {
            collections.push(name.clone());
            collections.sort();
            Self::save_collections(&mut tx, &collections)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn collection_metadata(&self, name: &str) -> Result<Option<CollectionMeta>, CassieError> {
        let name = self.canonical_collection_name(name);
        let tx = self.begin_schema_readonly_tx()?;
        if let Some(metadata) = Self::load_collection_metadata_from_tx(&tx, &name)? {
            return Ok(Some(metadata));
        }
        let exists = tx
            .get(&Self::collection_schema_key(&name))
            .map_err(CassieError::from)?
            .is_some();
        Ok(exists.then(|| CollectionMeta::new(&name, None)))
    }

    pub(crate) fn storage_mode_for_collection(
        &self,
        collection: &str,
    ) -> Result<CollectionStorageMode, CassieError> {
        Ok(self
            .collection_metadata(collection)?
            .map_or(CollectionStorageMode::RowStore, |metadata| {
                metadata.storage_mode
            }))
    }

    pub(crate) fn collection_uses_column_store(
        &self,
        collection: &str,
    ) -> Result<bool, CassieError> {
        Ok(self
            .storage_mode_for_collection(collection)?
            .uses_column_store_storage())
    }

    pub(crate) fn load_collection_metadata_from_tx(
        tx: &cntryl_midge::Transaction,
        name: &str,
    ) -> Result<Option<CollectionMeta>, CassieError> {
        let Some(raw) = tx
            .get(&Self::collection_metadata_key(name))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let mut metadata: CollectionMeta = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid collection metadata: {error}")))?;
        metadata.name = name.to_string();
        Ok(Some(metadata))
    }

    pub(crate) fn save_collection_metadata_to_tx(
        tx: &mut cntryl_midge::Transaction,
        metadata: &CollectionMeta,
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::collection_metadata_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn delete_collection_metadata_to_tx(
        tx: &mut cntryl_midge::Transaction,
        name: &str,
    ) -> Result<(), CassieError> {
        tx.delete(Self::collection_metadata_key(name))
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn rename_collection_metadata_to_tx(
        tx: &mut cntryl_midge::Transaction,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let metadata = Self::load_collection_metadata_from_tx(tx, current_name)?
            .unwrap_or_else(|| CollectionMeta::new(current_name, None));
        tx.delete(Self::collection_metadata_key(current_name))
            .map_err(CassieError::from)?;
        let mut renamed = metadata;
        renamed.name = next_name.to_string();
        Self::save_collection_metadata_to_tx(tx, &renamed)
    }

    pub(crate) fn load_column_store_document_from_tx(
        tx: &cntryl_midge::Transaction,
        _collection: &str,
        id: &str,
        row_schema: &RowSchema,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        if tx
            .get(&Self::column_store_row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?
            .is_none()
        {
            return Ok(None);
        }

        let mut payload = serde_json::Map::new();
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let Some(raw) = tx
                .get(&Self::column_store_field_key(
                    row_schema.relation_id,
                    field.field_id,
                    id,
                ))
                .map_err(CassieError::from)?
            else {
                continue;
            };
            let value = crate::midge::row_blob::decode_compact_value(&raw)?;
            payload.insert(field.name.clone(), value);
        }

        Ok(Some(serde_json::Value::Object(payload)))
    }

    pub(crate) fn write_column_store_document_to_tx(
        tx: &mut cntryl_midge::Transaction,
        _collection: &str,
        id: &str,
        payload: &serde_json::Value,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        let document = payload
            .as_object()
            .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;

        tx.put(
            Self::column_store_row_key(row_schema.relation_id, id),
            b"1".to_vec(),
            None,
        )
        .map_err(CassieError::from)?;
        tx.delete(Self::column_store_deleted_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?;

        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let key = Self::column_store_field_key(row_schema.relation_id, field.field_id, id);
            if let Some(value) = document.get(&field.name) {
                tx.put(
                    key,
                    crate::midge::row_blob::encode_compact_value(&field.data_type, value)?,
                    None,
                )
                .map_err(CassieError::from)?;
            } else {
                tx.delete(key).map_err(CassieError::from)?;
            }
        }
        Ok(())
    }

    pub(crate) fn delete_column_store_document_to_tx(
        tx: &mut cntryl_midge::Transaction,
        _collection: &str,
        id: &str,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        tx.delete(Self::column_store_row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?;
        tx.put(
            Self::column_store_deleted_key(row_schema.relation_id, id),
            b"1".to_vec(),
            None,
        )
        .map_err(CassieError::from)?;
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            tx.delete(Self::column_store_field_key(
                row_schema.relation_id,
                field.field_id,
                id,
            ))
            .map_err(CassieError::from)?;
        }
        Ok(())
    }

    pub(crate) fn scan_column_store_rows_batched(
        tx: &cntryl_midge::Transaction,
        request: ColumnStoreScanRequest<'_>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let scan_started = Instant::now();
        let mut row_decode = Duration::ZERO;
        let mut results = Vec::new();
        if request.limit == 0 {
            return Ok((
                results,
                MidgeScanTimings {
                    scan: scan_started.elapsed(),
                    row_decode,
                },
            ));
        }

        let mut current = Vec::with_capacity(request.batch_size.max(1));
        let mut emitted = 0usize;
        let row_prefix = Self::column_store_row_prefix(request.row_schema.relation_id);
        let scan = collect_scan(
            tx.scan(&Query::new().prefix(row_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;

        for (raw_key, _raw_value) in scan {
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &row_prefix) else {
                continue;
            };

            let decode_started = Instant::now();
            let payload = Self::project_column_store_document(
                tx,
                request.collection,
                &id,
                request.row_schema,
                request.projection,
                request.filter,
            )?;
            row_decode += decode_started.elapsed();
            let Some(payload) = payload else {
                continue;
            };

            current.push(DocumentRef { id, payload });
            emitted += 1;
            if current.len() >= request.batch_size.max(1) {
                results.push(current);
                current = Vec::with_capacity(request.batch_size.max(1));
            }
            if emitted >= request.limit {
                if !current.is_empty() {
                    results.push(current);
                }
                return Ok((
                    results,
                    MidgeScanTimings {
                        scan: scan_started.elapsed().saturating_sub(row_decode),
                        row_decode,
                    },
                ));
            }
        }

        if !current.is_empty() {
            results.push(current);
        }

        Ok((
            results,
            MidgeScanTimings {
                scan: scan_started.elapsed().saturating_sub(row_decode),
                row_decode,
            },
        ))
    }

    pub(crate) fn scan_ordered_column_store_rows_batched_by_id(
        tx: &cntryl_midge::Transaction,
        request: OrderedColumnStoreScanRequest<'_>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let scan_started = Instant::now();
        let mut row_decode = Duration::ZERO;
        let mut results = Vec::new();
        if request.limit == 0 {
            return Ok((
                results,
                MidgeScanTimings {
                    scan: scan_started.elapsed(),
                    row_decode,
                },
            ));
        }

        let row_prefix = Self::column_store_row_prefix(request.row_schema.relation_id);
        let mut ids = Vec::new();
        let scan = collect_scan(
            tx.scan(&Query::new().prefix(row_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;
        for (raw_key, _raw_value) in scan {
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &row_prefix) else {
                continue;
            };
            if Self::within_ordered_bounds(&id, request.start_bound, request.end_bound) {
                ids.push(id);
            }
        }
        ids.sort();
        if request.reverse {
            ids.reverse();
        }

        let mut current = Vec::with_capacity(request.batch_size.max(1));
        for id in ids.into_iter().take(request.limit) {
            let decode_started = Instant::now();
            let payload = Self::project_column_store_document(
                tx,
                request.collection,
                &id,
                request.row_schema,
                request.projection,
                None,
            )?;
            row_decode += decode_started.elapsed();
            let Some(payload) = payload else {
                continue;
            };
            current.push(DocumentRef { id, payload });
            if current.len() >= request.batch_size.max(1) {
                results.push(current);
                current = Vec::with_capacity(request.batch_size.max(1));
            }
        }

        if !current.is_empty() {
            results.push(current);
        }

        Ok((
            results,
            MidgeScanTimings {
                scan: scan_started.elapsed().saturating_sub(row_decode),
                row_decode,
            },
        ))
    }

    fn project_column_store_document(
        tx: &cntryl_midge::Transaction,
        _collection: &str,
        id: &str,
        row_schema: &RowSchema,
        projection: Option<&HashSet<String>>,
        filter: Option<&RowFilter>,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        if let Some(filter) = filter {
            let Some(filter_field) = row_schema
                .fields
                .iter()
                .find(|field| !field.retired && field.name.eq_ignore_ascii_case(&filter.field))
            else {
                return Ok(None);
            };
            let Some(raw) = tx
                .get(&Self::column_store_field_key(
                    row_schema.relation_id,
                    filter_field.field_id,
                    id,
                ))
                .map_err(CassieError::from)?
            else {
                return Ok(None);
            };
            let value = crate::midge::row_blob::decode_compact_value(&raw)?;
            if value != filter.value {
                return Ok(None);
            }
        }

        let mut object = serde_json::Map::new();
        for field in row_schema.fields.iter().filter(|field| !field.retired) {
            let include = projection
                .is_none_or(|projection| projection.contains(&field.name.to_ascii_lowercase()));
            if !include {
                continue;
            }
            let Some(raw) = tx
                .get(&Self::column_store_field_key(
                    row_schema.relation_id,
                    field.field_id,
                    id,
                ))
                .map_err(CassieError::from)?
            else {
                continue;
            };
            let value = crate::midge::row_blob::decode_compact_value(&raw)?;
            object.insert(field.name.clone(), value);
        }
        Ok(Some(serde_json::Value::Object(object)))
    }

    fn within_ordered_bounds(
        id: &str,
        start_bound: Option<&OrderedRowBound>,
        end_bound: Option<&OrderedRowBound>,
    ) -> bool {
        let start_ok = start_bound.is_none_or(|bound| {
            if bound.inclusive {
                id >= bound.id.as_str()
            } else {
                id > bound.id.as_str()
            }
        });
        let end_ok = end_bound.is_none_or(|bound| {
            if bound.inclusive {
                id <= bound.id.as_str()
            } else {
                id < bound.id.as_str()
            }
        });
        start_ok && end_ok
    }

    fn collection_metadata_key(name: &str) -> Vec<u8> {
        key_encoding::collection_metadata_key(name)
    }

    pub(crate) fn column_store_collection_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::column_store_collection_prefix(relation_id)
    }

    pub(crate) fn column_store_row_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::column_store_row_prefix(relation_id)
    }

    pub(crate) fn column_store_row_key(relation_id: u64, id: &str) -> Vec<u8> {
        key_encoding::column_store_row_key(relation_id, id)
    }

    fn column_store_deleted_key(relation_id: u64, id: &str) -> Vec<u8> {
        key_encoding::column_store_deleted_key(relation_id, id)
    }

    fn column_store_field_key(relation_id: u64, field_id: u32, id: &str) -> Vec<u8> {
        key_encoding::column_store_field_key(relation_id, field_id, id)
    }
}
