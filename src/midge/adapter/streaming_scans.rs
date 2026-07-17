use super::{
    decode_projected_row, decode_projected_row_with_aliases, decode_row, key_encoding, CassieError,
    DocumentRef, HashSet, Midge, Query, RowDecode,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

mod accounted_page;

use accounted_page::provisional_document_bytes;
pub(crate) use accounted_page::{AccountedDocument, AccountedDocumentPage};

const STORAGE_SCAN_PAGE_ENTRIES: usize = 256;

pub(crate) struct MidgeRowCursor {
    tx: cntryl_midge::Transaction,
    prefix: Vec<u8>,
    last_key: Option<Vec<u8>>,
    row_schema: crate::midge::row_blob::RowSchema,
    projection: Option<HashSet<String>>,
    include_historical_aliases: bool,
    exhausted: bool,
    pending: Option<AccountedDocument>,
    last_key_memory: Option<QueryMemoryReservation>,
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
    pub(crate) fn next_accounted_document(
        &mut self,
        midge: &Midge,
        controls: &QueryExecutionControls,
    ) -> Result<Option<AccountedDocument>, CassieError> {
        let mut documents = self.next_accounted_documents(midge, 1, controls)?;
        Ok(documents.pop())
    }

    pub(crate) fn next_documents(
        &mut self,
        midge: &Midge,
        limit: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Vec<DocumentRef>, CassieError> {
        let page = self.next_accounted_documents(midge, limit, controls)?;
        Ok(page
            .into_iter()
            .map(AccountedDocument::into_unaccounted_document)
            .collect())
    }

    pub(crate) fn next_page(
        &mut self,
        midge: &Midge,
        max_rows: usize,
        controls: &QueryExecutionControls,
    ) -> Result<(Vec<DocumentRef>, bool), CassieError> {
        let mut page = self.next_accounted_page(midge, max_rows, controls)?;
        let has_more = page.has_more();
        let mut documents = Vec::with_capacity(page.len().min(STORAGE_SCAN_PAGE_ENTRIES));
        while let Some(document) = page.pop_document() {
            documents.push(document.into_unaccounted_document());
        }
        debug_assert!(page.is_empty());
        Ok((documents, has_more))
    }

    pub(crate) fn next_accounted_page(
        &mut self,
        midge: &Midge,
        max_rows: usize,
        controls: &QueryExecutionControls,
    ) -> Result<AccountedDocumentPage, CassieError> {
        check_controls(controls)?;
        let fetch_rows = max_rows.saturating_add(1);
        let mut documents = self.next_accounted_documents(midge, fetch_rows, controls)?;
        let has_more = documents.len() > max_rows;
        if has_more {
            self.pending = documents.pop();
        }
        Ok(AccountedDocumentPage::from_documents(documents, has_more))
    }

    fn next_accounted_documents(
        &mut self,
        midge: &Midge,
        limit: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Vec<AccountedDocument>, CassieError> {
        check_controls(controls)?;
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut documents = Vec::new();
        if let Some(pending) = self.pending.take() {
            push_accounted_document(&mut documents, pending)?;
        }
        while !self.exhausted && documents.len() < limit {
            self.read_storage_page(midge, limit, controls, &mut documents)?;
        }
        Ok(documents)
    }

    fn read_storage_page(
        &mut self,
        midge: &Midge,
        result_limit: usize,
        controls: &QueryExecutionControls,
        documents: &mut Vec<AccountedDocument>,
    ) -> Result<(), CassieError> {
        check_controls(controls)?;
        let remaining = result_limit.saturating_sub(documents.len());
        let storage_limit = remaining.clamp(1, STORAGE_SCAN_PAGE_ENTRIES);
        let query_key_bytes = self.prefix.len().saturating_add(
            self.last_key
                .as_ref()
                .map_or(0, |key| key.len().saturating_add(1)),
        );
        let _query_key_memory = controls.reserve_query_memory(query_key_bytes)?;
        let mut query = Query::new()
            .prefix(self.prefix.clone().into())
            .limit(storage_limit);
        if let Some(last_key) = self.last_key.as_ref() {
            let mut next_key = last_key.clone();
            next_key.push(0);
            query = query.start_key(next_key.into());
        }
        let mut scan = self.tx.scan(&query).map_err(CassieError::from)?;
        let mut scanned_entries = 0usize;

        while documents.len() < result_limit {
            check_controls(controls)?;
            let Some(entry) = scan.next() else {
                if scanned_entries < storage_limit {
                    self.exhausted = true;
                }
                break;
            };
            check_controls(controls)?;
            let (raw_key, raw_value) = entry.map_err(CassieError::from)?;
            scanned_entries = scanned_entries.saturating_add(1);
            midge.record_query_scan_entry();
            if super::query_scan_control::should_cancel_controlled_query_scan() {
                return Err(CassieError::QueryCancelled);
            }

            let next_key_memory = controls.reserve_query_memory(raw_key.len())?;
            let next_key = raw_key.to_vec();
            self.last_key = Some(next_key);
            self.last_key_memory = Some(next_key_memory);

            let retained_bytes = provisional_document_bytes(
                &self.row_schema,
                self.projection.as_ref(),
                self.include_historical_aliases,
                raw_key.len(),
                raw_value.len(),
            )?;
            let Some(document) =
                AccountedDocument::try_build_optional(controls, retained_bytes, || {
                    let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &self.prefix)
                    else {
                        return Ok(None);
                    };
                    if id.is_empty() {
                        return Ok(None);
                    }
                    let payload = match self.projection.as_ref() {
                        Some(projection) if self.include_historical_aliases => {
                            decode_projected_row_with_aliases(
                                &self.row_schema,
                                &raw_value,
                                projection,
                            )?
                        }
                        Some(projection) => {
                            decode_projected_row(&self.row_schema, &raw_value, projection)?
                        }
                        None => decode_row(&self.row_schema, &raw_value)?,
                    };
                    Ok(Some(DocumentRef { id, payload }))
                })?
            else {
                continue;
            };
            push_accounted_document(documents, document)?;
        }
        Ok(())
    }
}

fn push_accounted_document(
    documents: &mut Vec<AccountedDocument>,
    document: AccountedDocument,
) -> Result<(), CassieError> {
    documents.try_reserve_exact(1).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to retain controlled storage page: {error}"))
    })?;
    documents.push(document);
    Ok(())
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
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
            last_key_memory: None,
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
