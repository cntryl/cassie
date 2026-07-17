use super::{
    BTreeMap, Cassie, CassieError, CassieSession, DocumentRef, RowDecode, TransactionRowChange,
};
use crate::runtime::QueryExecutionControls;

#[path = "document_scans/session_cursor.rs"]
mod session_cursor;

pub(crate) use session_cursor::SessionRowCursor;

impl Cassie {
    pub(crate) fn open_session_row_cursor(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        decode: RowDecode,
        controls: &QueryExecutionControls,
    ) -> Result<Option<SessionRowCursor>, CassieError> {
        let Some(persisted) = self.midge.open_row_cursor(collection, decode.clone())? else {
            return Ok(None);
        };
        SessionRowCursor::new(session, collection, persisted, decode, controls).map(Some)
    }

    pub(crate) fn scan_documents_batched_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        self.scan_documents_batched_for_session_limit(session, collection, batch_size, None)
    }

    pub(crate) fn scan_documents_batched_for_session_limit(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
        limit: Option<usize>,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        let collection_changes = if let Some(session) = session {
            session.collection_changes_matching(collection)
        } else {
            BTreeMap::new()
        };
        if collection_changes.is_empty() {
            return self.midge.scan_rows_batched_limit(
                collection,
                batch_size,
                RowDecode::Full,
                limit,
            );
        }

        let mut rows = self
            .midge
            .scan_documents(collection)?
            .into_iter()
            .map(|document| (document.id.clone(), document))
            .collect::<BTreeMap<_, _>>();

        for (id, change) in collection_changes {
            match change {
                TransactionRowChange::Upsert(payload) => {
                    rows.insert(id.clone(), DocumentRef { id, payload });
                }
                TransactionRowChange::Delete => {
                    rows.remove(&id);
                }
            }
        }

        let batch_size = batch_size.max(1);
        let mut batches = Vec::new();
        let mut current = Vec::with_capacity(batch_size);
        let limit = limit.unwrap_or(usize::MAX);
        for document in rows.into_values().take(limit) {
            current.push(document);
            if current.len() >= batch_size {
                batches.push(current);
                current = Vec::with_capacity(batch_size);
            }
        }
        if !current.is_empty() {
            batches.push(current);
        }

        Ok(batches)
    }
}
