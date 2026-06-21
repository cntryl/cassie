use super::*;

impl Midge {
    pub fn put_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        let schema = self
            .collection_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let row_schema = self.row_schema(collection)?;

        Self::validate_document(&schema, &payload)?;
        let row_blob = encode_row(&row_schema, &payload)?;

        let doc_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let vector_indexes = self
            .list_vector_indexes()?
            .into_iter()
            .filter(|index| index.collection == collection)
            .collect::<Vec<_>>();
        let normalized_records = vector_indexes
            .iter()
            .map(|index| {
                Self::normalized_vector_record_from_value(
                    collection,
                    &index.field,
                    &doc_id,
                    index.metadata.dimensions,
                    &index.metadata.metric,
                    payload.get(&index.field),
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut tx = self.begin_data_rw_tx()?;
        Self::delete_normalized_vector_keys_for_document(&mut tx, collection, &doc_id)?;
        tx.put(Self::row_key(collection, &doc_id), row_blob, None)
            .map_err(CassieError::from)?;
        let legacy_key = Self::doc_key(collection, &doc_id);
        if tx.get(&legacy_key).map_err(CassieError::from)?.is_some() {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        Self::write_normalized_vector_records(&mut tx, &normalized_records)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(doc_id)
    }

    pub fn get_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<DocumentRef>, CassieError> {
        let row_schema = self.row_schema(collection)?;

        let tx = self.begin_data_readonly_tx()?;
        let payload = match tx
            .get(&Self::row_key(collection, id))
            .map_err(CassieError::from)?
        {
            Some(payload) => Some(payload),
            None => tx
                .get(&Self::doc_key(collection, id))
                .map_err(CassieError::from)?,
        };

        let Some(payload) = payload else {
            return Ok(None);
        };
        let payload = decode_row(&row_schema, &payload)?;

        Ok(Some(DocumentRef {
            id: id.to_string(),
            payload,
        }))
    }

    pub fn delete_document(&self, collection: &str, id: &str) -> Result<bool, CassieError> {
        let _row_schema = self.row_schema(collection)?;

        let key = Self::row_key(collection, id);
        let legacy_key = Self::doc_key(collection, id);
        let mut tx = self.begin_data_rw_tx()?;
        let row_exists = tx.get(&key).map_err(CassieError::from)?.is_some();
        let legacy_exists = tx.get(&legacy_key).map_err(CassieError::from)?.is_some();
        if row_exists {
            tx.delete(key).map_err(CassieError::from)?;
        }
        if legacy_exists {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        let normalized_exists =
            Self::delete_normalized_vector_keys_for_document(&mut tx, collection, id)?;
        if row_exists || legacy_exists || normalized_exists {
            tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
            return Ok(true);
        }

        tx.rollback().map_err(CassieError::from)?;
        Ok(false)
    }

    pub fn scan_documents_batched(
        &self,
        collection: &str,
        batch_size: usize,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        self.scan_rows_batched(collection, batch_size, RowDecode::Full, None, None)
            .map(|(rows, _)| rows)
    }

    pub fn scan_rows_for_rebuild(
        &self,
        collection: &str,
        decode: RowDecode,
    ) -> Result<Vec<DocumentRef>, CassieError> {
        self.scan_rows_batched(collection, 1024, decode, None, None)
            .map(|(batches, _)| batches.into_iter().flatten().collect())
    }

    pub fn scan_rows_batched_limit(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
        limit: Option<usize>,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        self.scan_rows_batched(collection, batch_size, decode, None, limit)
            .map(|(rows, _)| rows)
    }

    pub fn scan_rows_batched_limit_with_timings(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        self.scan_rows_batched(collection, batch_size, decode, None, limit)
    }

    pub fn scan_projected_rows_batched_filter_limit_with_timings(
        &self,
        collection: &str,
        batch_size: usize,
        fields: Vec<String>,
        filter: Option<&RowFilter>,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        self.scan_rows_batched(
            collection,
            batch_size,
            RowDecode::Projected(fields),
            filter,
            limit,
        )
    }

    fn scan_rows_batched(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
        filter: Option<&RowFilter>,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let scan_started = Instant::now();
        let mut row_decode = Duration::ZERO;
        let row_schema = self.row_schema(collection)?;
        let projection = match decode {
            RowDecode::Full => None,
            RowDecode::Projected(fields) => Some(
                fields
                    .into_iter()
                    .map(|field| field.to_ascii_lowercase())
                    .collect::<HashSet<_>>(),
            ),
        };

        let tx = self.begin_data_readonly_tx()?;
        let batch_size = batch_size.max(1);
        let limit = limit.unwrap_or(usize::MAX);
        let mut results = Vec::new();
        if limit == 0 {
            return Ok((
                results,
                MidgeScanTimings {
                    scan: scan_started.elapsed(),
                    row_decode,
                },
            ));
        }
        let mut current = Vec::with_capacity(batch_size);
        let mut seen_ids = HashSet::new();
        let mut emitted = 0usize;

        for (prefix, needle, include_seen) in [
            (
                Self::row_prefix(collection),
                format!("r/{collection}/"),
                true,
            ),
            (
                Self::doc_prefix(collection),
                format!("doc:{collection}:"),
                false,
            ),
        ] {
            let mut iter = tx
                .scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?;
            while let Some((raw_key, raw_value)) = iter.next() {
                let raw_key = String::from_utf8(raw_key).map_err(|error| {
                    CassieError::Parse(format!("invalid document key in storage: {error}"))
                })?;
                let id = raw_key.strip_prefix(&needle).unwrap_or("").to_string();
                if id.is_empty() || (!include_seen && seen_ids.contains(&id)) {
                    continue;
                }
                seen_ids.insert(id.clone());

                let decode_started = Instant::now();
                let payload = match (projection.as_ref(), filter) {
                    (Some(projection), Some(filter)) => decode_projected_row_matching(
                        &row_schema,
                        &raw_value,
                        projection,
                        &filter.field,
                        &filter.value,
                    )?,
                    (Some(projection), None) => {
                        Some(decode_projected_row(&row_schema, &raw_value, projection)?)
                    }
                    (None, _) => Some(decode_row(&row_schema, &raw_value)?),
                };
                row_decode += decode_started.elapsed();
                let Some(payload) = payload else {
                    continue;
                };
                current.push(DocumentRef { id, payload });
                emitted += 1;
                if current.len() >= batch_size {
                    results.push(current);
                    current = Vec::with_capacity(batch_size);
                }
                if emitted >= limit {
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

    pub fn scan_documents(&self, collection: &str) -> Result<Vec<DocumentRef>, CassieError> {
        self.scan_documents_batched(collection, 1024)
            .map(|batches| batches.into_iter().flatten().collect())
    }

    pub fn all_fields_json(
        &self,
        collection: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, CassieError> {
        self.scan_documents(collection)
            .map(|docs| docs.into_iter().map(|doc| (doc.id, doc.payload)).collect())
    }

    fn validate_document(schema: &Schema, payload: &serde_json::Value) -> Result<(), CassieError> {
        let map = payload
            .as_object()
            .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;

        for field in &schema.fields {
            if let Some(value) = map.get(&field.name) {
                if let DataType::Vector(dim) = field.data_type {
                    if let Some(arr) = value.as_array() {
                        if arr.len() != dim {
                            return Err(CassieError::InvalidVector(format!(
                                "field '{}' expects vector({}) but got {}",
                                field.name,
                                dim,
                                arr.len()
                            )));
                        }
                    } else {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{}' expects vector({}) but received non-array",
                            field.name, dim
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}
