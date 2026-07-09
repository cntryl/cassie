use super::{
    decode_projected_row, decode_projected_row_matching_with_aliases,
    decode_projected_row_with_aliases, decode_row, encode_row, key_encoding, CassieError,
    ColumnStoreScanRequest, DataType, DocumentRef, HashSet, IndexKind, Instant, Midge,
    MidgeScanTimings, Query, RowDecode, RowFilter, RowSchema, Schema, Uuid, WriteOptions,
};
use std::time::Duration;

#[path = "documents/ordered_scan.rs"]
mod ordered_scan;
pub(crate) use ordered_scan::OrderedRowScanRequest;

#[derive(Debug)]
pub(crate) enum DocumentWriteOp {
    Put {
        id: String,
        payload: serde_json::Value,
    },
    Delete {
        id: String,
    },
}

#[derive(Debug, Default)]
pub(crate) struct DocumentWriteBatchReport {
    pub ids: Vec<String>,
    pub row_delta: i64,
    pub stats: crate::runtime::ProjectionWriteStats,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DocumentWriteBatchOptions {
    pub commit: WriteOptions,
    pub refresh_after_commit: bool,
}

impl DocumentWriteBatchOptions {
    pub(crate) fn sync() -> Self {
        Self {
            commit: WriteOptions::sync(),
            refresh_after_commit: true,
        }
    }

    pub(crate) fn buffered() -> Self {
        Self {
            commit: WriteOptions::buffered(),
            refresh_after_commit: true,
        }
    }
}

#[derive(Debug)]
struct PreparedWrite {
    id: String,
    row_blob: Option<Vec<u8>>,
    payload: Option<serde_json::Value>,
    normalized_records: Vec<crate::embeddings::NormalizedVectorRecord>,
}

struct DocumentWriteBatchContext {
    schema: Schema,
    row_schema: RowSchema,
    uses_column_store: bool,
    vector_indexes: Vec<crate::embeddings::VectorIndexRecord>,
    vector_fields: Vec<String>,
    scalar_indexes: Vec<super::IndexMeta>,
    time_series_indexes: Vec<super::IndexMeta>,
    graph: Option<crate::catalog::GraphMeta>,
    needs_existing_payload: bool,
}

struct ExistingDocumentState {
    payload: Option<serde_json::Value>,
    row_exists: bool,
    legacy_exists: bool,
}

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        let doc_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        self.apply_document_write_batch(
            collection,
            vec![DocumentWriteOp::Put {
                id: doc_id.clone(),
                payload,
            }],
        )?;
        Ok(doc_id)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_documents(
        &self,
        collection: &str,
        documents: Vec<(Option<String>, serde_json::Value)>,
    ) -> Result<Vec<String>, CassieError> {
        let ops = documents
            .into_iter()
            .map(|(id, payload)| DocumentWriteOp::Put {
                id: id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                payload,
            })
            .collect::<Vec<_>>();
        let report = self.apply_document_write_batch(collection, ops)?;
        Ok(report.ids)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<DocumentRef>, CassieError> {
        let row_schema = self.row_schema(collection)?;
        if self.collection_uses_column_store(collection)? {
            let tx = self.begin_data_readonly_tx()?;
            let Some(payload) =
                Self::load_column_store_document_from_tx(&tx, collection, id, &row_schema)?
            else {
                return Ok(None);
            };
            return Ok(Some(DocumentRef {
                id: id.to_string(),
                payload,
            }));
        }

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

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_document(&self, collection: &str, id: &str) -> Result<bool, CassieError> {
        let _row_schema = self.row_schema(collection)?;
        let report = self.apply_document_write_batch(
            collection,
            vec![DocumentWriteOp::Delete { id: id.to_string() }],
        )?;
        Ok(report.row_delta < 0)
    }

    pub(crate) fn put_document_with_stats(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
    ) -> Result<(String, crate::runtime::ProjectionWriteStats, i64), CassieError> {
        let doc_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let report = self.apply_document_write_batch(
            collection,
            vec![DocumentWriteOp::Put {
                id: doc_id.clone(),
                payload,
            }],
        )?;
        Ok((doc_id, report.stats, report.row_delta))
    }

    pub(crate) fn delete_document_with_stats(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<(bool, crate::runtime::ProjectionWriteStats, i64), CassieError> {
        let report = self.apply_document_write_batch(
            collection,
            vec![DocumentWriteOp::Delete { id: id.to_string() }],
        )?;
        Ok((report.row_delta < 0, report.stats, report.row_delta))
    }

    pub(crate) fn apply_document_write_batch(
        &self,
        collection: &str,
        operations: Vec<DocumentWriteOp>,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        self.apply_document_write_batch_with_options(
            collection,
            operations,
            DocumentWriteBatchOptions::sync(),
        )
    }

    pub(crate) fn apply_document_write_batch_with_options(
        &self,
        collection: &str,
        operations: Vec<DocumentWriteOp>,
        options: DocumentWriteBatchOptions,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        if operations.is_empty() {
            return Ok(DocumentWriteBatchReport::default());
        }
        let context = self.document_write_batch_context(collection)?;
        let prepared = Self::prepare_document_writes(collection, operations, &context)?;
        let mut tx = self.begin_data_rw_tx()?;
        let mut report = DocumentWriteBatchReport::default();
        for prepared in prepared {
            Self::apply_prepared_document_write(
                &mut tx,
                collection,
                &context,
                prepared,
                &mut report,
            )?;
        }
        self.finish_document_write_batch(collection, options, tx, report)
    }

    fn document_write_batch_context(
        &self,
        collection: &str,
    ) -> Result<DocumentWriteBatchContext, CassieError> {
        let schema = self
            .collection_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let row_schema = self.row_schema(collection)?;
        let uses_column_store = self.collection_uses_column_store(collection)?;
        let vector_indexes = self
            .list_vector_indexes()?
            .into_iter()
            .filter(|index| index.collection == collection)
            .collect::<Vec<_>>();
        let vector_fields = vector_indexes
            .iter()
            .map(|index| index.field.clone())
            .collect::<Vec<_>>();
        let scalar_indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::Scalar)
            .collect::<Vec<_>>();
        let time_series_indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::TimeSeries)
            .collect::<Vec<_>>();
        let graph = self.graph_for_edge_collection(collection)?;
        let needs_existing_payload =
            !scalar_indexes.is_empty() || !time_series_indexes.is_empty() || graph.is_some();
        Ok(DocumentWriteBatchContext {
            schema,
            row_schema,
            uses_column_store,
            vector_indexes,
            vector_fields,
            scalar_indexes,
            time_series_indexes,
            graph,
            needs_existing_payload,
        })
    }

    fn prepare_document_writes(
        collection: &str,
        operations: Vec<DocumentWriteOp>,
        context: &DocumentWriteBatchContext,
    ) -> Result<Vec<PreparedWrite>, CassieError> {
        let mut prepared = Vec::with_capacity(operations.len());
        for operation in operations {
            match operation {
                DocumentWriteOp::Put { id, payload } => {
                    Self::validate_document(&context.schema, &payload)?;
                    let row_blob = encode_row(&context.row_schema, &payload)?;
                    let normalized_records = context
                        .vector_indexes
                        .iter()
                        .map(|index| {
                            Self::normalized_vector_record_from_value(
                                collection,
                                &index.field,
                                &id,
                                index.metadata.dimensions,
                                index.metadata.metric,
                                payload.get(&index.field),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>();

                    prepared.push(PreparedWrite {
                        id,
                        row_blob: Some(row_blob),
                        payload: Some(payload),
                        normalized_records,
                    });
                }
                DocumentWriteOp::Delete { id } => {
                    prepared.push(PreparedWrite {
                        id,
                        row_blob: None,
                        payload: None,
                        normalized_records: Vec::new(),
                    });
                }
            }
        }
        Ok(prepared)
    }

    fn apply_prepared_document_write(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        prepared: PreparedWrite,
        report: &mut DocumentWriteBatchReport,
    ) -> Result<(), CassieError> {
        let existing = Self::existing_document_state_for_prepared_write(
            tx,
            collection,
            context,
            &prepared.id,
        )?;
        if prepared.row_blob.is_some() {
            return Self::apply_prepared_put(tx, collection, context, prepared, &existing, report);
        }
        Self::apply_prepared_delete(tx, collection, context, &prepared, &existing, report)
    }

    fn existing_document_state_for_prepared_write(
        tx: &cntryl_midge::Transaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        id: &str,
    ) -> Result<ExistingDocumentState, CassieError> {
        let legacy_key = Self::doc_key(collection, id);
        if context.uses_column_store {
            let payload = if context.needs_existing_payload {
                Self::load_column_store_document_from_tx(tx, collection, id, &context.row_schema)?
            } else {
                None
            };
            let row_exists = if context.needs_existing_payload {
                payload.is_some()
            } else {
                tx.get(&Self::column_store_row_key(collection, id))
                    .map_err(CassieError::from)?
                    .is_some()
            };
            let legacy_exists = tx.get(&legacy_key).map_err(CassieError::from)?.is_some();
            return Ok(ExistingDocumentState {
                payload,
                row_exists,
                legacy_exists,
            });
        }

        let row_raw = tx
            .get(&Self::row_key(collection, id))
            .map_err(CassieError::from)?;
        let legacy_raw = tx.get(&legacy_key).map_err(CassieError::from)?;
        let payload = if context.needs_existing_payload {
            match (row_raw.as_ref(), legacy_raw.as_ref()) {
                (Some(raw), _) | (None, Some(raw)) => Some(decode_row(&context.row_schema, raw)?),
                (None, None) => None,
            }
        } else {
            None
        };
        Ok(ExistingDocumentState {
            payload,
            row_exists: row_raw.is_some(),
            legacy_exists: legacy_raw.is_some(),
        })
    }

    fn apply_prepared_put(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        prepared: PreparedWrite,
        existing: &ExistingDocumentState,
        report: &mut DocumentWriteBatchReport,
    ) -> Result<(), CassieError> {
        let row_blob = prepared
            .row_blob
            .expect("prepared put operation must include row blob");
        let payload = prepared
            .payload
            .expect("prepared put operation must include payload");
        let row_key = Self::row_key(collection, &prepared.id);
        let legacy_key = Self::doc_key(collection, &prepared.id);
        let replacing = existing.row_exists || existing.legacy_exists;
        let normalized_deleted = Self::delete_normalized_vector_keys_for_document(
            tx,
            collection,
            &prepared.id,
            &context.vector_fields,
        )?;
        report.stats.index_deletes = report
            .stats
            .index_deletes
            .saturating_add(u64::try_from(normalized_deleted).unwrap_or(0));
        if context.uses_column_store {
            Self::write_column_store_document_to_tx(
                tx,
                collection,
                &prepared.id,
                &payload,
                &context.schema,
            )?;
        } else {
            tx.put(row_key, row_blob, None).map_err(CassieError::from)?;
        }
        Self::write_document_hash_to_tx(
            tx,
            collection,
            &prepared.id,
            &context.row_schema,
            &payload,
        )?;
        if existing.legacy_exists {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        Self::write_normalized_vector_records(tx, &prepared.normalized_records)?;
        let index_changes = Self::sync_secondary_indexes_for_write(
            tx,
            context,
            &prepared.id,
            existing.payload.as_ref(),
            Some(&payload),
        )?;
        report.ids.push(prepared.id);
        report.stats.row_puts = report.stats.row_puts.saturating_add(1);
        report.stats.index_puts = report.stats.index_puts.saturating_add(
            u64::try_from(
                prepared
                    .normalized_records
                    .len()
                    .saturating_add(index_changes.1),
            )
            .unwrap_or(0),
        );
        report.stats.index_deletes = report
            .stats
            .index_deletes
            .saturating_add(u64::try_from(index_changes.0).unwrap_or(0));
        report.stats.metadata_puts = report.stats.metadata_puts.saturating_add(1);
        if !replacing {
            report.row_delta = report.row_delta.saturating_add(1);
        }
        Ok(())
    }

    fn apply_prepared_delete(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        prepared: &PreparedWrite,
        existing: &ExistingDocumentState,
        report: &mut DocumentWriteBatchReport,
    ) -> Result<(), CassieError> {
        let row_key = Self::row_key(collection, &prepared.id);
        let legacy_key = Self::doc_key(collection, &prepared.id);
        if existing.row_exists && context.uses_column_store {
            Self::delete_column_store_document_to_tx(
                tx,
                collection,
                &prepared.id,
                &context.schema,
            )?;
        } else if existing.row_exists {
            tx.delete(row_key).map_err(CassieError::from)?;
        }
        if existing.legacy_exists {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        if existing.row_exists || existing.legacy_exists {
            Self::delete_document_hash_to_tx(tx, collection, &prepared.id)?;
            report.stats.metadata_deletes = report.stats.metadata_deletes.saturating_add(1);
            report.stats.row_deletes = report.stats.row_deletes.saturating_add(1);
            report.row_delta = report.row_delta.saturating_sub(1);
        }
        let normalized_deleted = Self::delete_normalized_vector_keys_for_document(
            tx,
            collection,
            &prepared.id,
            &context.vector_fields,
        )?;
        let index_changes = Self::sync_secondary_indexes_for_write(
            tx,
            context,
            &prepared.id,
            existing.payload.as_ref(),
            None,
        )?;
        report.stats.index_deletes = report.stats.index_deletes.saturating_add(
            u64::try_from(normalized_deleted.saturating_add(index_changes.0)).unwrap_or(0),
        );
        report.stats.index_puts = report
            .stats
            .index_puts
            .saturating_add(u64::try_from(index_changes.1).unwrap_or(0));
        Ok(())
    }

    fn sync_secondary_indexes_for_write(
        tx: &mut cntryl_midge::Transaction,
        context: &DocumentWriteBatchContext,
        id: &str,
        previous: Option<&serde_json::Value>,
        next: Option<&serde_json::Value>,
    ) -> Result<(usize, usize), CassieError> {
        let (scalar_deleted, scalar_puts) = Self::sync_scalar_indexes_for_document(
            tx,
            id,
            previous,
            next,
            &context.scalar_indexes,
        )?;
        let (time_series_deleted, time_series_puts) = Self::sync_time_series_indexes_for_document(
            tx,
            id,
            previous,
            next,
            &context.time_series_indexes,
        )?;
        let (graph_deleted, graph_puts) = Self::sync_graph_adjacency_for_document(
            tx,
            context.graph.as_ref(),
            id,
            previous,
            next,
        )?;
        Ok((
            scalar_deleted
                .saturating_add(time_series_deleted)
                .saturating_add(graph_deleted),
            scalar_puts
                .saturating_add(time_series_puts)
                .saturating_add(graph_puts),
        ))
    }

    fn finish_document_write_batch(
        &self,
        collection: &str,
        options: DocumentWriteBatchOptions,
        tx: cntryl_midge::Transaction,
        mut report: DocumentWriteBatchReport,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        let changed = report.stats.row_puts > 0
            || report.stats.row_deletes > 0
            || report.stats.index_puts > 0
            || report.stats.index_deletes > 0
            || report.stats.metadata_puts > 0
            || report.stats.metadata_deletes > 0;
        if !changed {
            tx.rollback().map_err(CassieError::from)?;
            return Ok(report);
        }

        tx.commit(options.commit).map_err(CassieError::from)?;
        report.stats.batch_flushes = report.stats.batch_flushes.saturating_add(1);

        if options.refresh_after_commit {
            let _ = self.rebuild_column_batches_for_collection(collection)?;
            self.refresh_hnsw_indexes_for_collection(collection)?;
            self.refresh_ivfflat_indexes_for_collection(collection)?;
            self.refresh_projection_hashes_after_write(collection, report.row_delta)?;
        }
        Ok(report)
    }

    pub(crate) fn flush_data_family(&self) -> Result<(), CassieError> {
        let layout = self.ensure_families_ready()?;
        self.engine
            .flush_cf(&layout.data)
            .map_err(CassieError::from)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn scan_documents_batched(
        &self,
        collection: &str,
        batch_size: usize,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        self.scan_rows_batched(collection, batch_size, RowDecode::Full, None, None)
            .map(|(rows, _)| rows)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn scan_rows_for_rebuild(
        &self,
        collection: &str,
        decode: RowDecode,
    ) -> Result<Vec<DocumentRef>, CassieError> {
        self.scan_rows_batched(collection, 1024, decode, None, None)
            .map(|(batches, _)| batches.into_iter().flatten().collect())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
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

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn scan_rows_batched_limit_with_timings(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        self.scan_rows_batched(collection, batch_size, decode, None, limit)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
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
            RowDecode::ProjectedHistorical(fields),
            filter,
            limit,
        )
    }

    pub(super) fn scan_rows_batched(
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
        let (projection, include_historical_aliases) = decode.into_projection();

        let tx = self.begin_data_readonly_tx()?;
        let batch_size = batch_size.max(1);
        let limit = limit.unwrap_or(usize::MAX);
        if self.collection_uses_column_store(collection)? {
            return Self::scan_column_store_rows_batched(
                &tx,
                ColumnStoreScanRequest {
                    collection,
                    row_schema: &row_schema,
                    batch_size,
                    projection: projection.as_ref(),
                    filter,
                    limit,
                },
            );
        }
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

        for (prefix, include_seen) in [
            (Self::row_prefix(collection), true),
            (Self::doc_prefix(collection), false),
        ] {
            let iter = tx
                .scan(&Query::new().prefix(prefix.clone().into()))
                .map_err(CassieError::from)?;
            for (raw_key, raw_value) in iter {
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) else {
                    continue;
                };
                if id.is_empty() || (!include_seen && seen_ids.contains(&id)) {
                    continue;
                }
                seen_ids.insert(id.clone());

                let decode_started = Instant::now();
                let payload = match (projection.as_ref(), filter) {
                    (Some(projection), Some(filter)) => decode_projected_row_matching_with_aliases(
                        &row_schema,
                        &raw_value,
                        projection,
                        &filter.field,
                        &filter.value,
                        include_historical_aliases,
                    )?,
                    (Some(projection), None) => Some(if include_historical_aliases {
                        decode_projected_row_with_aliases(&row_schema, &raw_value, projection)?
                    } else {
                        decode_projected_row(&row_schema, &raw_value, projection)?
                    }),
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

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn scan_documents(&self, collection: &str) -> Result<Vec<DocumentRef>, CassieError> {
        self.scan_documents_batched(collection, 1024)
            .map(|batches| batches.into_iter().flatten().collect())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn all_fields_json(
        &self,
        collection: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, CassieError> {
        self.scan_documents(collection)
            .map(|docs| docs.into_iter().map(|doc| (doc.id, doc.payload)).collect())
    }

    pub(super) fn validate_document(
        schema: &Schema,
        payload: &serde_json::Value,
    ) -> Result<(), CassieError> {
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
