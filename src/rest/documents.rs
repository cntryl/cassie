use crate::app::{Cassie, CassieError};
use serde_json::Value;

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn create(cassie: &Cassie, collection: &str, body: &[u8]) -> Result<Value, CassieError> {
    let document: Value =
        serde_json::from_slice(body).map_err(|e| CassieError::Parse(e.to_string()))?;

    let id = cassie.ingest_document(collection, document)?;

    Ok(serde_json::json!({ "id": id }))
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn get(cassie: &Cassie, collection: &str, id: &str) -> Result<Value, CassieError> {
    let doc = cassie.midge.get_document(collection, id)?;

    Ok(match doc {
        Some(document) => document.payload,
        None => {
            return Err(CassieError::NotFound("document not found".to_string()));
        }
    })
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn delete(cassie: &Cassie, collection: &str, id: &str) -> Result<Value, CassieError> {
    let removed = cassie.delete_document_for_session(None, collection, id)?;

    Ok(serde_json::json!({"deleted": removed}))
}
