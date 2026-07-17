use super::{Cassie, CassieError};
use crate::runtime::QueryCancellationHandle;

pub(super) struct DocumentWriteRequest<'a> {
    pub(super) id: Option<String>,
    pub(super) payload: serde_json::Value,
    pub(super) apply_defaults: bool,
    pub(super) exclude_id: Option<&'a str>,
    pub(super) cancellation: Option<&'a QueryCancellationHandle>,
}

impl<'a> DocumentWriteRequest<'a> {
    pub(super) fn new(
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&'a str>,
    ) -> Self {
        Self {
            id,
            payload,
            apply_defaults,
            exclude_id,
            cancellation: None,
        }
    }

    fn with_cancellation(mut self, cancellation: &'a QueryCancellationHandle) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
}

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn ingest_document(
        &self,
        collection: &str,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        self.write_document(collection, None, payload, true, None)
    }

    pub(crate) fn write_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<String, CassieError> {
        self.write_document_for_session_with_cancellation(
            None,
            collection,
            DocumentWriteRequest::new(id, payload, apply_defaults, exclude_id),
        )
    }

    /// Writes one document while honoring caller-controlled cancellation at publication.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or cancellation fails.
    pub fn ingest_document_with_cancellation(
        &self,
        collection: &str,
        payload: serde_json::Value,
        cancellation: &QueryCancellationHandle,
    ) -> Result<String, CassieError> {
        self.write_document_for_session_with_cancellation(
            None,
            collection,
            DocumentWriteRequest::new(None, payload, true, None).with_cancellation(cancellation),
        )
    }

    /// Deletes one document while honoring caller-controlled cancellation at publication.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or cancellation fails.
    pub fn delete_document_with_cancellation(
        &self,
        collection: &str,
        id: &str,
        cancellation: &QueryCancellationHandle,
    ) -> Result<bool, CassieError> {
        self.delete_document_for_session_with_cancellation(None, collection, id, Some(cancellation))
    }
}

pub(super) fn check_document_cancellation(
    cancellation: Option<&QueryCancellationHandle>,
) -> Result<(), CassieError> {
    if cancellation.is_some_and(QueryCancellationHandle::is_cancelled) {
        return Err(CassieError::QueryCancelled);
    }
    Ok(())
}
