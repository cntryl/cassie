use super::*;
use std::time::Duration;

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

#[derive(Debug, Clone)]
struct OrderedScanEntry {
    id: String,
    raw_value: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderedScanSelection {
    Row,
    Doc,
    Both,
}

impl Midge {
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

        #[derive(Debug)]
        struct PreparedWrite {
            id: String,
            row_blob: Option<Vec<u8>>,
            payload: Option<serde_json::Value>,
            normalized_records: Vec<crate::embeddings::NormalizedVectorRecord>,
        }

        let mut prepared = Vec::with_capacity(operations.len());
        for operation in operations {
            match operation {
                DocumentWriteOp::Put { id, payload } => {
                    Self::validate_document(&schema, &payload)?;
                    let row_blob = encode_row(&row_schema, &payload)?;
                    let normalized_records = vector_indexes
                        .iter()
                        .map(|index| {
                            Self::normalized_vector_record_from_value(
                                collection,
                                &index.field,
                                &id,
                                index.metadata.dimensions,
                                &index.metadata.metric,
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

        let mut tx = self.begin_data_rw_tx()?;
        let mut report = DocumentWriteBatchReport::default();

        for prepared in prepared {
            let row_key = Self::row_key(collection, &prepared.id);
            let legacy_key = Self::doc_key(collection, &prepared.id);
            let existing_payload = if !needs_existing_payload {
                None
            } else if uses_column_store {
                Self::load_column_store_document_from_tx(
                    &tx,
                    collection,
                    &prepared.id,
                    &row_schema,
                )?
            } else {
                Self::load_document_payload_from_tx(&tx, collection, &prepared.id, &row_schema)?
            };

            if let Some(row_blob) = prepared.row_blob {
                let payload = prepared
                    .payload
                    .expect("prepared put operation must include payload");
                let row_exists = if uses_column_store {
                    tx.get(&Self::column_store_row_key(collection, &prepared.id))
                        .map_err(CassieError::from)?
                        .is_some()
                } else {
                    tx.get(&row_key).map_err(CassieError::from)?.is_some()
                };
                let legacy_exists = tx.get(&legacy_key).map_err(CassieError::from)?.is_some();
                let replacing = row_exists || legacy_exists;

                let normalized_deleted = Self::delete_normalized_vector_keys_for_document(
                    &mut tx,
                    collection,
                    &prepared.id,
                    &vector_fields,
                )?;
                report.stats.index_deletes = report
                    .stats
                    .index_deletes
                    .saturating_add(u64::try_from(normalized_deleted).unwrap_or(0));

                if uses_column_store {
                    Self::write_column_store_document_to_tx(
                        &mut tx,
                        collection,
                        &prepared.id,
                        &payload,
                        &schema,
                    )?;
                } else {
                    tx.put(row_key, row_blob, None).map_err(CassieError::from)?;
                }
                Self::write_document_hash_to_tx(
                    &mut tx,
                    collection,
                    &prepared.id,
                    &row_schema,
                    &payload,
                )?;
                if tx.get(&legacy_key).map_err(CassieError::from)?.is_some() {
                    tx.delete(legacy_key).map_err(CassieError::from)?;
                }
                Self::write_normalized_vector_records(&mut tx, &prepared.normalized_records)?;
                let (scalar_deleted, scalar_puts) = self.sync_scalar_indexes_for_document(
                    &mut tx,
                    &prepared.id,
                    existing_payload.as_ref(),
                    Some(&payload),
                    &scalar_indexes,
                )?;
                let (time_series_deleted, time_series_puts) = self
                    .sync_time_series_indexes_for_document(
                        &mut tx,
                        &prepared.id,
                        existing_payload.as_ref(),
                        Some(&payload),
                        &time_series_indexes,
                    )?;
                let (graph_deleted, graph_puts) = self.sync_graph_adjacency_for_document(
                    &mut tx,
                    graph.as_ref(),
                    &prepared.id,
                    existing_payload.as_ref(),
                    Some(&payload),
                )?;

                report.ids.push(prepared.id.clone());
                report.stats.row_puts = report.stats.row_puts.saturating_add(1);
                report.stats.index_puts = report.stats.index_puts.saturating_add(
                    u64::try_from(
                        prepared
                            .normalized_records
                            .len()
                            .saturating_add(scalar_puts)
                            .saturating_add(time_series_puts)
                            .saturating_add(graph_puts),
                    )
                    .unwrap_or(0),
                );
                report.stats.index_deletes = report.stats.index_deletes.saturating_add(
                    u64::try_from(
                        scalar_deleted
                            .saturating_add(time_series_deleted)
                            .saturating_add(graph_deleted),
                    )
                    .unwrap_or(0),
                );
                report.stats.metadata_puts = report.stats.metadata_puts.saturating_add(1);
                if !replacing {
                    report.row_delta = report.row_delta.saturating_add(1);
                }
            } else {
                let row_exists = if uses_column_store {
                    tx.get(&Self::column_store_row_key(collection, &prepared.id))
                        .map_err(CassieError::from)?
                        .is_some()
                } else {
                    tx.get(&row_key).map_err(CassieError::from)?.is_some()
                };
                let legacy_exists = tx.get(&legacy_key).map_err(CassieError::from)?.is_some();

                if row_exists && uses_column_store {
                    Self::delete_column_store_document_to_tx(
                        &mut tx,
                        collection,
                        &prepared.id,
                        &schema,
                    )?;
                } else if row_exists {
                    tx.delete(row_key).map_err(CassieError::from)?;
                }
                if legacy_exists {
                    tx.delete(legacy_key).map_err(CassieError::from)?;
                }
                if row_exists || legacy_exists {
                    Self::delete_document_hash_to_tx(&mut tx, collection, &prepared.id)?;
                    report.stats.metadata_deletes = report.stats.metadata_deletes.saturating_add(1);
                    report.stats.row_deletes = report.stats.row_deletes.saturating_add(1);
                    report.row_delta = report.row_delta.saturating_sub(1);
                }

                let normalized_deleted = Self::delete_normalized_vector_keys_for_document(
                    &mut tx,
                    collection,
                    &prepared.id,
                    &vector_fields,
                )?;
                let (scalar_deleted, scalar_puts) = self.sync_scalar_indexes_for_document(
                    &mut tx,
                    &prepared.id,
                    existing_payload.as_ref(),
                    None,
                    &scalar_indexes,
                )?;
                let (time_series_deleted, time_series_puts) = self
                    .sync_time_series_indexes_for_document(
                        &mut tx,
                        &prepared.id,
                        existing_payload.as_ref(),
                        None,
                        &time_series_indexes,
                    )?;
                let (graph_deleted, graph_puts) = self.sync_graph_adjacency_for_document(
                    &mut tx,
                    graph.as_ref(),
                    &prepared.id,
                    existing_payload.as_ref(),
                    None,
                )?;
                report.stats.index_deletes = report.stats.index_deletes.saturating_add(
                    u64::try_from(
                        normalized_deleted
                            .saturating_add(scalar_deleted)
                            .saturating_add(time_series_deleted)
                            .saturating_add(graph_deleted),
                    )
                    .unwrap_or(0),
                );
                report.stats.index_puts = report.stats.index_puts.saturating_add(
                    u64::try_from(
                        scalar_puts
                            .saturating_add(time_series_puts)
                            .saturating_add(graph_puts),
                    )
                    .unwrap_or(0),
                );
            }
        }

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

    fn load_document_payload_from_tx(
        tx: &cntryl_midge::Transaction,
        collection: &str,
        id: &str,
        row_schema: &RowSchema,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        if let Some(raw) = tx
            .get(&Self::row_key(collection, id))
            .map_err(CassieError::from)?
        {
            return decode_row(row_schema, &raw).map(Some);
        }
        let Some(raw) = tx
            .get(&Self::doc_key(collection, id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        decode_row(row_schema, &raw).map(Some)
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
            RowDecode::ProjectedHistorical(fields),
            filter,
            limit,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn scan_ordered_rows_batched_by_id_limit_with_timings(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
        start_bound: Option<super::OrderedRowBound>,
        end_bound: Option<super::OrderedRowBound>,
        reverse: bool,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        self.scan_ordered_rows_batched_by_id(
            collection,
            batch_size,
            decode,
            start_bound,
            end_bound,
            reverse,
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
            return self.scan_column_store_rows_batched(
                &tx,
                collection,
                &row_schema,
                batch_size,
                projection.as_ref(),
                include_historical_aliases,
                filter,
                limit,
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
            let mut iter = tx
                .scan(&Query::new().prefix(prefix.clone().into()))
                .map_err(CassieError::from)?;
            while let Some((raw_key, raw_value)) = iter.next() {
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

    #[allow(clippy::too_many_arguments)]
    fn scan_ordered_rows_batched_by_id(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
        start_bound: Option<super::OrderedRowBound>,
        end_bound: Option<super::OrderedRowBound>,
        reverse: bool,
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
            return self.scan_ordered_column_store_rows_batched_by_id(
                &tx,
                collection,
                &row_schema,
                batch_size,
                projection.as_ref(),
                include_historical_aliases,
                start_bound,
                end_bound,
                reverse,
                limit,
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

        let row_prefix = Self::row_prefix(collection);
        let doc_prefix = Self::doc_prefix(collection);

        let mut row_iter = tx
            .scan(&Self::ordered_row_query(
                &row_prefix,
                start_bound.as_ref(),
                end_bound.as_ref(),
                reverse,
            ))
            .map_err(CassieError::from)?;
        let mut doc_iter = tx
            .scan(&Self::ordered_row_query(
                &doc_prefix,
                start_bound.as_ref(),
                end_bound.as_ref(),
                reverse,
            ))
            .map_err(CassieError::from)?;

        let mut row_next = Self::ordered_next_entry(&mut row_iter, &row_prefix)?;
        let mut doc_next = Self::ordered_next_entry(&mut doc_iter, &doc_prefix)?;
        let mut current = Vec::with_capacity(batch_size);
        let mut emitted = 0usize;

        while row_next.is_some() || doc_next.is_some() {
            let Some(selection) =
                Self::ordered_selection(row_next.as_ref(), doc_next.as_ref(), reverse)
            else {
                break;
            };

            let selected = match selection {
                OrderedScanSelection::Row => row_next
                    .take()
                    .expect("row entry should exist for row selection"),
                OrderedScanSelection::Doc => doc_next
                    .take()
                    .expect("doc entry should exist for doc selection"),
                OrderedScanSelection::Both => row_next
                    .take()
                    .expect("row entry should exist for duplicate selection"),
            };

            let decode_started = Instant::now();
            let payload = match projection.as_ref() {
                Some(projection) if include_historical_aliases => {
                    decode_projected_row_with_aliases(&row_schema, &selected.raw_value, projection)?
                }
                Some(projection) => {
                    decode_projected_row(&row_schema, &selected.raw_value, projection)?
                }
                None => decode_row(&row_schema, &selected.raw_value)?,
            };
            row_decode += decode_started.elapsed();

            current.push(DocumentRef {
                id: selected.id,
                payload,
            });
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

            match selection {
                OrderedScanSelection::Row => {
                    row_next = Self::ordered_next_entry(&mut row_iter, &row_prefix)?;
                }
                OrderedScanSelection::Doc => {
                    doc_next = Self::ordered_next_entry(&mut doc_iter, &doc_prefix)?;
                }
                OrderedScanSelection::Both => {
                    row_next = Self::ordered_next_entry(&mut row_iter, &row_prefix)?;
                    doc_next = Self::ordered_next_entry(&mut doc_iter, &doc_prefix)?;
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

    fn ordered_row_query(
        prefix: &[u8],
        start_bound: Option<&super::OrderedRowBound>,
        end_bound: Option<&super::OrderedRowBound>,
        reverse: bool,
    ) -> Query {
        let mut query = Query::new().prefix(prefix.to_vec().into());
        if let Some(bound) = start_bound {
            query =
                query.start_key(Self::ordered_start_key(prefix, &bound.id, bound.inclusive).into());
        }
        if let Some(bound) = end_bound {
            query = query.end_key(Self::ordered_end_key(prefix, &bound.id, bound.inclusive).into());
        }
        if reverse {
            query = query.reverse();
        }
        query
    }

    fn ordered_start_key(prefix: &[u8], id: &str, inclusive: bool) -> Vec<u8> {
        let mut key = prefix.to_vec();
        key.extend_from_slice(id.as_bytes());
        if !inclusive {
            key.push(0);
        }
        key
    }

    fn ordered_end_key(prefix: &[u8], id: &str, inclusive: bool) -> Vec<u8> {
        let mut key = prefix.to_vec();
        key.extend_from_slice(id.as_bytes());
        if inclusive {
            key.push(0);
        }
        key
    }

    fn ordered_next_entry(
        iter: &mut cntryl_midge::ScanIterator,
        prefix: &[u8],
    ) -> Result<Option<OrderedScanEntry>, CassieError> {
        while let Some((raw_key, raw_value)) = iter.next() {
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, prefix) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            return Ok(Some(OrderedScanEntry { id, raw_value }));
        }

        Ok(None)
    }

    fn ordered_selection(
        row: Option<&OrderedScanEntry>,
        doc: Option<&OrderedScanEntry>,
        reverse: bool,
    ) -> Option<OrderedScanSelection> {
        match (row, doc) {
            (Some(_), None) => Some(OrderedScanSelection::Row),
            (None, Some(_)) => Some(OrderedScanSelection::Doc),
            (Some(row), Some(doc)) => match row.id.cmp(&doc.id) {
                std::cmp::Ordering::Less => {
                    if reverse {
                        Some(OrderedScanSelection::Doc)
                    } else {
                        Some(OrderedScanSelection::Row)
                    }
                }
                std::cmp::Ordering::Greater => {
                    if reverse {
                        Some(OrderedScanSelection::Row)
                    } else {
                        Some(OrderedScanSelection::Doc)
                    }
                }
                std::cmp::Ordering::Equal => Some(OrderedScanSelection::Both),
            },
            (None, None) => None,
        }
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
