use super::{Cassie, VectorIndexRecord, QueryError, DataType, VectorIndexType, HnswIndexOptions, VectorIndexMetadata, DistanceMetric};

pub(super) fn vector_index_metadata(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> Result<VectorIndexRecord, QueryError> {
    let schema = cassie
        .midge
        .collection_schema(&statement.table)
        .ok_or_else(|| {
            QueryError::General(format!(
                "collection '{}' not found while creating vector index",
                statement.table
            ))
        })?;

    let vector_field = schema
        .fields
        .iter()
        .find(|field| {
            statement
                .fields
                .first()
                .is_some_and(|value| field.name == *value)
        })
        .ok_or_else(|| {
            let field = statement.fields.first().cloned().unwrap_or_default();
            QueryError::General(format!(
                "index field '{}' does not exist in collection '{}'",
                field, statement.table
            ))
        })?;

    let DataType::Vector(dimensions) = vector_field.data_type else {
            return Err(QueryError::General(format!(
                "field '{}' is not a vector field",
                vector_field.name
            )));
        };
    if cassie.embedding_provider.dimensions() != dimensions {
        return Err(QueryError::General(format!(
            "embedding dimension mismatch: field '{}' has {}, active provider '{}' model '{}' has {}",
            vector_field.name,
            dimensions,
            cassie.embedding_provider.provider_name(),
            cassie.embedding_provider.model_name(),
            cassie.embedding_provider.dimensions()
        )));
    }

    let source_field = statement
        .options
        .get("source_field")
        .ok_or_else(|| {
            QueryError::General("CREATE INDEX USING vector requires source_field".to_string())
        })?.clone();

    let source_metadata = schema
        .fields
        .iter()
        .find(|field| field.name == source_field)
        .ok_or_else(|| {
            QueryError::General(format!(
                "source field '{}' does not exist in collection '{}'",
                source_field, statement.table
            ))
        })?;

    if !matches!(source_metadata.data_type, DataType::Text | DataType::Json) {
        return Err(QueryError::General(format!(
            "source field '{source_field}' must be text/json for vector index"
        )));
    }

    let index_type = match statement
        .options
        .get("index_type")
        .map_or("bruteforce", String::as_str)
    {
        "hnsw" => VectorIndexType::Hnsw,
        "ivfflat" => VectorIndexType::IvfFlat,
        _ => VectorIndexType::BruteForce,
    };
    let hnsw = if index_type == VectorIndexType::Hnsw {
        Some(HnswIndexOptions {
            version: 1,
            m: statement
                .options
                .get("m")
                .and_then(|value| value.parse().ok())
                .unwrap_or(16),
            ef_construction: statement
                .options
                .get("ef_construction")
                .and_then(|value| value.parse().ok())
                .unwrap_or(64),
            ef_search: statement
                .options
                .get("ef_search")
                .and_then(|value| value.parse().ok())
                .unwrap_or(40),
        })
    } else {
        None
    };
    let ivfflat = if index_type == VectorIndexType::IvfFlat {
        Some(crate::embeddings::IvfFlatIndexOptions {
            version: 1,
            lists: statement
                .options
                .get("lists")
                .and_then(|value| value.parse().ok())
                .unwrap_or(64),
            probes: statement
                .options
                .get("probes")
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
            training_sample_size: statement
                .options
                .get("training_sample_size")
                .and_then(|value| value.parse().ok())
                .unwrap_or(2_560),
            training_seed: statement
                .options
                .get("training_seed")
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
        })
    } else {
        None
    };

    let metadata = VectorIndexMetadata {
        provider: cassie.embedding_provider.provider_name().to_string(),
        model: cassie.embedding_provider.model_name().to_string(),
        dimensions,
        metric: statement
            .options
            .get("metric")
            .and_then(|metric| metric.parse::<DistanceMetric>().ok())
            .unwrap_or(DistanceMetric::Cosine),
        index_type,
        hnsw,
        ivfflat,
        ivfflat_training: None,
    };

    Ok(VectorIndexRecord {
        collection: statement.table.clone(),
        field: statement.fields.first().cloned().unwrap_or_default(),
        source_field,
        metadata,
    })
}
