use std::collections::HashMap;

use crate::app::Cassie;
use crate::app::CassieError;
use crate::embeddings::{DistanceMetric, VectorIndexMetadata, VectorIndexRecord};
use crate::types::DataType;
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

pub fn create(cassie: &Cassie, collection: &str, body: &[u8]) -> Result<Value, CassieError> {
    let payload: CreateIndexRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;

    let kind = payload.kind.to_lowercase();
    if kind != "vector" {
        return Err(CassieError::Unsupported(format!(
            "index kind '{}' is not supported",
            payload.kind
        )));
    }

    let schema = cassie
        .midge
        .collection_schema(collection)
        .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;

    let source_field = payload
        .options
        .get("source_field")
        .cloned()
        .ok_or_else(|| {
            CassieError::InvalidEmbedding("index options.source_field is required".to_string())
        })?;

    let vector_field = schema
        .fields
        .iter()
        .find(|field| field.name == payload.field)
        .ok_or_else(|| {
            CassieError::InvalidEmbedding(format!(
                "vector field '{}' does not exist in collection '{}'",
                payload.field, collection
            ))
        })?;

    let vector_dimensions = if let DataType::Vector(dim) = vector_field.data_type.clone() {
        dim
    } else {
        return Err(CassieError::InvalidEmbedding(format!(
            "field '{}' is not a vector field",
            payload.field
        )));
    };

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
    };

    if metadata.dimensions != vector_dimensions {
        return Err(CassieError::InvalidEmbedding(format!(
            "vector index metadata dimension mismatch: collection field '{}' has {} but provider '{}', model '{}' has {}",
            payload.field,
            vector_dimensions,
            metadata.provider,
            metadata.model,
            metadata.dimensions
        )));
    }

    if let Some(existing) = cassie.catalog.get_vector_index(collection, &payload.field) {
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

        return Ok(serde_json::json!({
            "collection": collection,
            "field": payload.field,
            "source_field": source_field,
            "provider": metadata.provider,
            "model": metadata.model,
            "dimensions": metadata.dimensions,
            "metric": metadata.metric.as_str(),
            "status": "exists",
        }));
    }

    let source_field_metadata = schema
        .fields
        .iter()
        .find(|field| field.name == source_field)
        .ok_or_else(|| {
            CassieError::InvalidEmbedding(format!(
                "source field '{}' does not exist in collection '{}'",
                source_field, collection
            ))
        })?;

    if !matches!(
        source_field_metadata.data_type,
        DataType::Text | DataType::Json
    ) {
        return Err(CassieError::InvalidEmbedding(format!(
            "source field '{}' must be text/json for embedding index",
            source_field
        )));
    }

    let record = VectorIndexRecord {
        collection: collection.to_string(),
        field: payload.field.clone(),
        source_field,
        metadata,
    };

    cassie.midge.put_vector_index(record.clone())?;
    cassie.register_vector_index(record.clone());

    Ok(serde_json::json!({
        "collection": collection,
        "field": payload.field,
        "source_field": record.source_field,
        "provider": record.metadata.provider,
        "model": record.metadata.model,
        "dimensions": record.metadata.dimensions,
        "metric": record.metadata.metric.as_str(),
        "status": "created",
    }))
}
