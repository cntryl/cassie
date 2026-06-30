use super::{
    Cassie, DataType, DistanceMetric, HnswIndexOptions, QueryError, VectorIndexMetadata,
    VectorIndexRecord, VectorIndexType,
};

pub(super) fn vector_index_metadata(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> Result<VectorIndexRecord, QueryError> {
    let schema = collection_schema(cassie, &statement.table)?;
    let vector_field = vector_field(&schema, statement)?;
    let dimensions = vector_dimensions(cassie, vector_field)?;
    let source_field = source_field(statement)?;
    validate_source_field(&schema, &statement.table, &source_field)?;

    let index_type = vector_index_type(statement);
    let hnsw = hnsw_options(statement, &index_type);
    let ivfflat = ivfflat_options(statement, &index_type);

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

fn collection_schema(
    cassie: &Cassie,
    collection: &str,
) -> Result<crate::types::Schema, QueryError> {
    cassie.midge.collection_schema(collection).ok_or_else(|| {
        QueryError::General(format!(
            "collection '{collection}' not found while creating vector index"
        ))
    })
}

fn vector_field<'a>(
    schema: &'a crate::types::Schema,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> Result<&'a crate::types::FieldSchema, QueryError> {
    schema
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
        })
}

fn vector_dimensions(
    cassie: &Cassie,
    vector_field: &crate::types::FieldSchema,
) -> Result<usize, QueryError> {
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
    Ok(dimensions)
}

fn source_field(statement: &crate::sql::ast::CreateIndexStatement) -> Result<String, QueryError> {
    statement
        .options
        .get("source_field")
        .cloned()
        .ok_or_else(|| {
            QueryError::General("CREATE INDEX USING vector requires source_field".to_string())
        })
}

fn validate_source_field(
    schema: &crate::types::Schema,
    collection: &str,
    source_field: &str,
) -> Result<(), QueryError> {
    let source_metadata = schema
        .fields
        .iter()
        .find(|field| field.name == source_field)
        .ok_or_else(|| {
            QueryError::General(format!(
                "source field '{source_field}' does not exist in collection '{collection}'"
            ))
        })?;

    if !matches!(source_metadata.data_type, DataType::Text | DataType::Json) {
        return Err(QueryError::General(format!(
            "source field '{source_field}' must be text/json for vector index"
        )));
    }
    Ok(())
}

fn vector_index_type(statement: &crate::sql::ast::CreateIndexStatement) -> VectorIndexType {
    match statement
        .options
        .get("index_type")
        .map_or("bruteforce", String::as_str)
    {
        "hnsw" => VectorIndexType::Hnsw,
        "ivfflat" => VectorIndexType::IvfFlat,
        _ => VectorIndexType::BruteForce,
    }
}

fn hnsw_options(
    statement: &crate::sql::ast::CreateIndexStatement,
    index_type: &VectorIndexType,
) -> Option<HnswIndexOptions> {
    (*index_type == VectorIndexType::Hnsw).then(|| HnswIndexOptions {
        version: 1,
        m: parse_option(statement, "m").unwrap_or(16),
        ef_construction: parse_option(statement, "ef_construction").unwrap_or(64),
        ef_search: parse_option(statement, "ef_search").unwrap_or(40),
    })
}

fn ivfflat_options(
    statement: &crate::sql::ast::CreateIndexStatement,
    index_type: &VectorIndexType,
) -> Option<crate::embeddings::IvfFlatIndexOptions> {
    (*index_type == VectorIndexType::IvfFlat).then(|| crate::embeddings::IvfFlatIndexOptions {
        version: 1,
        lists: parse_option(statement, "lists").unwrap_or(64),
        probes: parse_option(statement, "probes").unwrap_or(1),
        training_sample_size: parse_option(statement, "training_sample_size").unwrap_or(2_560),
        training_seed: parse_option(statement, "training_seed").unwrap_or(1),
    })
}

fn parse_option<T: std::str::FromStr>(
    statement: &crate::sql::ast::CreateIndexStatement,
    key: &str,
) -> Option<T> {
    statement
        .options
        .get(key)
        .and_then(|value| value.parse().ok())
}
