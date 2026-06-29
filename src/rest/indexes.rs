use std::collections::HashMap;

use crate::app::Cassie;
use crate::app::CassieError;
use crate::embeddings::{DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType};
use crate::types::{DataType, Schema};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct CreateIndexRequest {
    pub field: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub options: HashMap<String, String>,
}

fn default_kind() -> String {
    "vector".to_string()
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn create(cassie: &Cassie, collection: &str, body: &[u8]) -> Result<Value, CassieError> {
    let payload = parse_create_index_request(body)?;
    validate_index_kind(&payload)?;
    let schema = cassie
        .midge
        .collection_schema(collection)
        .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
    let source_field = required_source_field(&payload)?;
    let vector_dimensions = vector_field_dimensions(&schema, collection, &payload.field)?;
    let metadata = vector_index_metadata(cassie, &payload, vector_dimensions)?;

    if let Some(existing) =
        existing_vector_index_response(cassie, collection, &payload, &source_field, &metadata)?
    {
        return Ok(existing);
    }

    validate_source_field(&schema, collection, &source_field)?;

    let record = VectorIndexRecord {
        collection: collection.to_string(),
        field: payload.field.clone(),
        source_field,
        metadata,
    };

    cassie.midge.put_vector_index(record.clone())?;
    cassie.register_vector_index(record.clone());
    Ok(vector_index_response(collection, &record, "created"))
}

fn parse_create_index_request(body: &[u8]) -> Result<CreateIndexRequest, CassieError> {
    serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))
}

fn validate_index_kind(payload: &CreateIndexRequest) -> Result<(), CassieError> {
    if payload.kind.eq_ignore_ascii_case("vector") {
        return Ok(());
    }
    Err(CassieError::Unsupported(format!(
        "index kind '{}' is not supported",
        payload.kind
    )))
}

fn required_source_field(payload: &CreateIndexRequest) -> Result<String, CassieError> {
    payload.options.get("source_field").cloned().ok_or_else(|| {
        CassieError::InvalidEmbedding("index options.source_field is required".to_string())
    })
}

fn vector_field_dimensions(
    schema: &Schema,
    collection: &str,
    field: &str,
) -> Result<usize, CassieError> {
    let vector_field = schema
        .fields
        .iter()
        .find(|schema_field| schema_field.name == field)
        .ok_or_else(|| {
            CassieError::InvalidEmbedding(format!(
                "vector field '{field}' does not exist in collection '{collection}'"
            ))
        })?;

    match vector_field.data_type {
        DataType::Vector(dimensions) => Ok(dimensions),
        _ => Err(CassieError::InvalidEmbedding(format!(
            "field '{field}' is not a vector field"
        ))),
    }
}

fn vector_index_metadata(
    cassie: &Cassie,
    payload: &CreateIndexRequest,
    vector_dimensions: usize,
) -> Result<VectorIndexMetadata, CassieError> {
    let metric = payload
        .options
        .get("metric")
        .and_then(|metric| metric.parse::<DistanceMetric>().ok())
        .unwrap_or(DistanceMetric::Cosine);
    let metadata = VectorIndexMetadata {
        provider: cassie.embedding_provider.provider_name().to_string(),
        model: cassie.embedding_provider.model_name().to_string(),
        dimensions: cassie.embedding_provider.dimensions(),
        metric,
        index_type: VectorIndexType::BruteForce,
        hnsw: None,
        ivfflat: None,
        ivfflat_training: None,
    };

    if metadata.dimensions != vector_dimensions {
        return Err(CassieError::InvalidEmbedding(format!(
            "vector index metadata dimension mismatch: collection field '{}' has {} but provider '{}', model '{}' has {}",
            payload.field, vector_dimensions, metadata.provider, metadata.model, metadata.dimensions
        )));
    }

    Ok(metadata)
}

fn existing_vector_index_response(
    cassie: &Cassie,
    collection: &str,
    payload: &CreateIndexRequest,
    source_field: &str,
    metadata: &VectorIndexMetadata,
) -> Result<Option<Value>, CassieError> {
    let Some(existing) = cassie.catalog.get_vector_index(collection, &payload.field) else {
        return Ok(None);
    };

    if existing.source_field != source_field
        || existing.metadata.provider != metadata.provider
        || existing.metadata.model != metadata.model
        || existing.metadata.dimensions != metadata.dimensions
        || existing.metadata.metric != metadata.metric
    {
        return Err(CassieError::InvalidEmbedding(format!(
            "incompatible vector index redefinition for collection '{}' field '{}'",
            collection, payload.field
        )));
    }

    Ok(Some(vector_index_response(collection, &existing, "exists")))
}

fn validate_source_field(
    schema: &Schema,
    collection: &str,
    source_field: &str,
) -> Result<(), CassieError> {
    let source_field_metadata = schema
        .fields
        .iter()
        .find(|field| field.name == source_field)
        .ok_or_else(|| {
            CassieError::InvalidEmbedding(format!(
                "source field '{source_field}' does not exist in collection '{collection}'"
            ))
        })?;

    if matches!(
        source_field_metadata.data_type,
        DataType::Text | DataType::Json
    ) {
        return Ok(());
    }

    Err(CassieError::InvalidEmbedding(format!(
        "source field '{source_field}' must be text/json for embedding index"
    )))
}

fn vector_index_response(collection: &str, record: &VectorIndexRecord, status: &str) -> Value {
    serde_json::json!({
        "collection": collection,
        "field": record.field,
        "source_field": record.source_field,
        "provider": record.metadata.provider,
        "model": record.metadata.model,
        "dimensions": record.metadata.dimensions,
        "metric": record.metadata.metric.as_str(),
        "status": status,
    })
}
