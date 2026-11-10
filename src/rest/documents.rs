use crate::app::{Cassie, CassieError};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use serde_json::Value;

fn resolve_collection(cassie: &Cassie, collection: &str) -> Result<String, CassieError> {
    cassie
        .catalog
        .get_schema(collection)
        .map(|schema| schema.collection)
        .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn create(cassie: &Cassie, collection: &str, body: &[u8]) -> Result<Value, CassieError> {
    create_with_cancellation(cassie, collection, body, &QueryCancellationHandle::new())
}

#[doc(hidden)]
pub fn create_with_cancellation(
    cassie: &Cassie,
    collection: &str,
    body: &[u8],
    cancellation: &QueryCancellationHandle,
) -> Result<Value, CassieError> {
    let limits = cassie.runtime.limits();
    let controls = QueryExecutionControls::with_cancellation(
        &limits,
        std::time::Instant::now(),
        cancellation.clone(),
    );
    let parsed =
        crate::runtime::accounted::json_arena::parse_value(body, &controls).map_err(|error| {
            match error {
                crate::runtime::accounted::json_arena::ValueParseError::Invalid => {
                    CassieError::Parse(crate::runtime::accounted::json_arena::syntax_error(body))
                }
                crate::runtime::accounted::json_arena::ValueParseError::Controlled(error) => error,
            }
        })?;
    let (document, parse_memory) = parsed.into_parts();
    let collection = resolve_collection(cassie, collection)?;

    let id = cassie.ingest_document_with_cancellation(&collection, document, cancellation)?;
    drop(parse_memory);

    Ok(serde_json::json!({ "id": id }))
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn get(cassie: &Cassie, collection: &str, id: &str) -> Result<Value, CassieError> {
    let collection = resolve_collection(cassie, collection)?;
    let doc = cassie.midge.get_document(&collection, id)?;

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
    delete_with_cancellation(cassie, collection, id, &QueryCancellationHandle::new())
}

#[doc(hidden)]
pub fn delete_with_cancellation(
    cassie: &Cassie,
    collection: &str,
    id: &str,
    cancellation: &QueryCancellationHandle,
) -> Result<Value, CassieError> {
    let collection = resolve_collection(cassie, collection)?;
    let removed = cassie.delete_document_with_referential_actions(&collection, id, cancellation)?;

    Ok(serde_json::json!({"deleted": removed}))
}
