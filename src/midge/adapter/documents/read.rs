use super::super::{
    decode_projected_row, decode_projected_row_matching_with_aliases,
    decode_projected_row_with_aliases, decode_row, key_encoding, CassieError,
    ColumnStoreScanRequest, DocumentRef, HashSet, Instant, Midge, MidgeScanTimings, Query,
    RowDecode, RowFilter, RowSchema,
};
use std::time::Duration;

#[derive(Clone, Copy)]
struct RowBlobScanRequest<'a> {
    collection: &'a str,
    row_schema: &'a RowSchema,
    tx: &'a cntryl_midge::Transaction,
    batch_size: usize,
    projection: Option<&'a std::collections::HashSet<String>>,
    filter: Option<&'a RowFilter>,
    include_historical_aliases: bool,
    limit: usize,
    scan_started: Instant,
    row_decode: Duration,
}

impl Midge {
    pub(crate) fn flush_data_family(&self) -> Result<(), CassieError> {
        let family = self.database_family(&self.default_database)?;
        self.engine.flush_cf(&family).map_err(CassieError::from)
    }

    /// Returns the durable data epoch, or zero before the first changed write.
    ///
    /// # Errors
    ///
    /// Returns an error when the persisted epoch is malformed or cannot be read.
    pub fn data_epoch(&self) -> Result<u64, CassieError> {
        let tx = self.database_tx(
            &self.default_database,
            cntryl_midge::TransactionMode::ReadOnly,
        )?;
        Self::load_data_epoch_from_tx(&tx)
    }

    pub(crate) fn data_epoch_for_database(&self, database: &str) -> Result<u64, CassieError> {
        let tx = self.database_tx(database, cntryl_midge::TransactionMode::ReadOnly)?;
        Self::load_data_epoch_from_tx(&tx)
    }

    fn load_data_epoch_from_tx(tx: &cntryl_midge::Transaction) -> Result<u64, CassieError> {
        let Some(raw) = tx.get(&Self::data_epoch_key()).map_err(CassieError::from)? else {
            return Ok(0);
        };
        let bytes: [u8; 8] = raw
            .as_ref()
            .try_into()
            .map_err(|_| CassieError::Parse("invalid persisted data epoch".to_string()))?;
        Ok(u64::from_be_bytes(bytes))
    }

    /// Returns the durable generation for a collection, or zero before its first changed write.
    ///
    /// # Errors
    ///
    /// Returns an error when the persisted generation is malformed or cannot be read.
    pub fn collection_generation(&self, collection: &str) -> Result<u64, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::collection_generation_key(&collection))
            .map_err(CassieError::from)?
        else {
            return Ok(0);
        };
        let bytes: [u8; 8] = raw.as_ref().try_into().map_err(|_| {
            CassieError::Parse("invalid persisted collection generation".to_string())
        })?;
        Ok(u64::from_be_bytes(bytes))
    }

    pub(crate) fn increment_data_epoch_in_tx(
        tx: &mut cntryl_midge::Transaction,
    ) -> Result<u64, CassieError> {
        let next = match tx.get(&Self::data_epoch_key()).map_err(CassieError::from)? {
            Some(raw) => {
                let bytes: [u8; 8] = raw
                    .as_ref()
                    .try_into()
                    .map_err(|_| CassieError::Parse("invalid persisted data epoch".to_string()))?;
                u64::from_be_bytes(bytes).wrapping_add(1)
            }
            None => 1,
        };
        tx.put(Self::data_epoch_key(), next.to_be_bytes().to_vec(), None)
            .map_err(CassieError::from)?;
        Ok(next)
    }

    pub(crate) fn increment_collection_generation_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
    ) -> Result<u64, CassieError> {
        let key = Self::collection_generation_key(collection);
        let next = match tx.get(&key).map_err(CassieError::from)? {
            Some(raw) => {
                let bytes: [u8; 8] = raw.as_ref().try_into().map_err(|_| {
                    CassieError::Parse("invalid persisted collection generation".to_string())
                })?;
                u64::from_be_bytes(bytes).wrapping_add(1)
            }
            None => 1,
        };
        tx.put(key, next.to_be_bytes().to_vec(), None)
            .map_err(CassieError::from)?;
        Ok(next)
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

    pub(crate) fn scan_rows_batched(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
        filter: Option<&RowFilter>,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let scan_started = Instant::now();
        let row_decode = Duration::ZERO;
        let row_schema = self.row_schema(&collection)?;
        let (projection, include_historical_aliases) = decode.into_projection();

        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let batch_size = batch_size.max(1);
        let limit = limit.unwrap_or(usize::MAX);
        if self.collection_uses_column_store(&collection)? {
            return Self::scan_column_store_rows_batched(
                &tx,
                ColumnStoreScanRequest {
                    collection: &collection,
                    row_schema: &row_schema,
                    batch_size,
                    projection: projection.as_ref(),
                    filter,
                    limit,
                },
            );
        }
        if limit == 0 {
            return Ok((
                Vec::new(),
                MidgeScanTimings {
                    scan: scan_started.elapsed(),
                    row_decode,
                },
            ));
        }
        self.scan_row_blobs_batched(RowBlobScanRequest {
            collection: &collection,
            row_schema: &row_schema,
            tx: &tx,
            batch_size,
            projection: projection.as_ref(),
            filter,
            include_historical_aliases,
            limit,
            scan_started,
            row_decode,
        })
    }

    fn scan_row_blobs_batched(
        &self,
        request: RowBlobScanRequest<'_>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let RowBlobScanRequest {
            collection,
            row_schema,
            tx,
            batch_size,
            projection,
            filter,
            include_historical_aliases,
            limit,
            scan_started,
            mut row_decode,
        } = request;
        let mut results = Vec::new();
        let mut current = Vec::with_capacity(batch_size);
        let mut seen_ids = HashSet::new();
        let mut emitted = 0usize;

        for (prefix, include_seen) in [
            (Self::row_prefix(row_schema.relation_id), true),
            (Self::doc_prefix(collection), false),
        ] {
            let iter = tx
                .scan(&Query::new().prefix(prefix.clone().into()))
                .map_err(CassieError::from)?;
            for entry in iter {
                let (raw_key, raw_value) = entry.map_err(CassieError::from)?;
                self.record_query_scan_entry();
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) else {
                    continue;
                };
                if id.is_empty() || (!include_seen && seen_ids.contains(&id)) {
                    continue;
                }
                seen_ids.insert(id.clone());

                let decode_started = Instant::now();
                let payload = match (projection, filter) {
                    (Some(projection), Some(filter)) => decode_projected_row_matching_with_aliases(
                        row_schema,
                        &raw_value,
                        projection,
                        &filter.field,
                        &filter.value,
                        include_historical_aliases,
                    )?,
                    (Some(projection), None) => Some(if include_historical_aliases {
                        decode_projected_row_with_aliases(row_schema, &raw_value, projection)?
                    } else {
                        decode_projected_row(row_schema, &raw_value, projection)?
                    }),
                    (None, _) => Some(decode_row(row_schema, &raw_value)?),
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
}
