use super::{
    decode_projected_row, decode_projected_row_with_aliases, decode_row, key_encoding, CassieError,
    DocumentRef, HashSet, Midge, Query, RowDecode,
};
use crate::runtime::QueryExecutionControls;

pub(crate) struct MidgeRowCursor {
    tx: cntryl_midge::Transaction,
    prefix: Vec<u8>,
    last_key: Option<Vec<u8>>,
    row_schema: crate::midge::row_blob::RowSchema,
    projection: Option<HashSet<String>>,
    include_historical_aliases: bool,
    exhausted: bool,
    pending: Option<DocumentRef>,
}

impl std::fmt::Debug for MidgeRowCursor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MidgeRowCursor")
            .field("prefix", &self.prefix)
            .field("last_key", &self.last_key)
            .field("exhausted", &self.exhausted)
            .field("has_pending", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

impl MidgeRowCursor {
    pub(crate) fn next_documents(
        &mut self,
        midge: &Midge,
        limit: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Vec<DocumentRef>, CassieError> {
        if self.exhausted || limit == 0 {
            return Ok(self.pending.take().into_iter().take(limit).collect());
        }
        let mut documents = Vec::with_capacity(limit);
        if let Some(pending) = self.pending.take() {
            documents.push(pending);
            if documents.len() == limit {
                return Ok(documents);
            }
        }
        let mut query = Query::new().prefix(self.prefix.clone().into());
        if let Some(last_key) = self.last_key.as_ref() {
            let mut next_key = last_key.clone();
            next_key.push(0);
            query = query.start_key(next_key.into());
        }
        let mut scan = self.tx.scan(&query).map_err(CassieError::from)?;
        while documents.len() < limit {
            if controls.is_cancelled() {
                return Err(CassieError::QueryCancelled);
            }
            if controls.is_timed_out() {
                return Err(CassieError::DeadlineExceeded);
            }
            let Some(entry) = scan.next() else {
                self.exhausted = true;
                break;
            };
            let (raw_key, raw_value) = entry.map_err(CassieError::from)?;
            midge.record_query_scan_entry();
            self.last_key = Some(raw_key.to_vec());
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &self.prefix) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let payload = match self.projection.as_ref() {
                Some(projection) if self.include_historical_aliases => {
                    decode_projected_row_with_aliases(&self.row_schema, &raw_value, projection)?
                }
                Some(projection) => decode_projected_row(&self.row_schema, &raw_value, projection)?,
                None => decode_row(&self.row_schema, &raw_value)?,
            };
            documents.push(DocumentRef { id, payload });
        }
        Ok(documents)
    }

    pub(crate) fn next_page(
        &mut self,
        midge: &Midge,
        max_rows: usize,
        controls: &QueryExecutionControls,
    ) -> Result<(Vec<DocumentRef>, bool), CassieError> {
        let mut documents = self.next_documents(midge, max_rows.saturating_add(1), controls)?;
        let has_more = documents.len() > max_rows;
        if has_more {
            self.pending = documents.pop();
        }
        Ok((documents, has_more))
    }
}

impl Midge {
    pub(crate) fn open_row_cursor(
        &self,
        collection: &str,
        decode: RowDecode,
    ) -> Result<Option<MidgeRowCursor>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        if self.collection_uses_column_store(&collection)? {
            return Ok(None);
        }
        let row_schema = self.row_schema(&collection)?;
        let prefix = Self::row_prefix(row_schema.relation_id);
        let (projection, include_historical_aliases) = decode.into_projection();
        Ok(Some(MidgeRowCursor {
            tx: self.begin_data_readonly_tx_for(&collection)?,
            prefix,
            last_key: None,
            row_schema,
            projection,
            include_historical_aliases,
            exhausted: false,
            pending: None,
        }))
    }

    pub(crate) fn scan_rows_until<E, F>(
        &self,
        collection: &str,
        decode: RowDecode,
        mut visit: F,
    ) -> Result<usize, E>
    where
        E: From<CassieError>,
        F: FnMut(DocumentRef) -> Result<bool, E>,
    {
        let collection = self.canonical_collection_name(collection);
        if self
            .collection_uses_column_store(&collection)
            .map_err(E::from)?
        {
            let (batches, _) = self
                .scan_rows_batched(&collection, 1024, decode, None, None)
                .map_err(E::from)?;
            let mut emitted = 0usize;
            for document in batches.into_iter().flatten() {
                emitted += 1;
                if !visit(document)? {
                    break;
                }
            }
            return Ok(emitted);
        }

        let row_schema = self.row_schema(&collection).map_err(E::from)?;
        let (projection, include_historical_aliases) = decode.into_projection();
        let tx = self
            .begin_data_readonly_tx_for(&collection)
            .map_err(E::from)?;
        let mut seen_ids = HashSet::new();
        let mut emitted = 0usize;

        for (prefix, include_seen) in [
            (Self::row_prefix(row_schema.relation_id), true),
            (Self::doc_prefix(&collection), false),
        ] {
            let scan = tx
                .scan(&Query::new().prefix(prefix.clone().into()))
                .map_err(CassieError::from)
                .map_err(E::from)?;
            for entry in scan {
                let (raw_key, raw_value) = entry.map_err(CassieError::from).map_err(E::from)?;
                self.record_query_scan_entry();
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) else {
                    continue;
                };
                if id.is_empty() || (!include_seen && seen_ids.contains(&id)) {
                    continue;
                }
                seen_ids.insert(id.clone());

                let payload = match projection.as_ref() {
                    Some(projection) if include_historical_aliases => {
                        decode_projected_row_with_aliases(&row_schema, &raw_value, projection)
                            .map_err(E::from)?
                    }
                    Some(projection) => decode_projected_row(&row_schema, &raw_value, projection)
                        .map_err(E::from)?,
                    None => decode_row(&row_schema, &raw_value).map_err(E::from)?,
                };
                emitted += 1;
                if !visit(DocumentRef { id, payload })? {
                    return Ok(emitted);
                }
            }
        }

        Ok(emitted)
    }
}
