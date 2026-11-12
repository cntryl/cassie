use super::{Cassie, DataType, DistanceMetric, QueryError, VectorIndexMetadata, VectorIndexRecord};
use crate::vector::index_options::normalize_vector_index_options;

pub(super) fn vector_index_metadata(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> Result<VectorIndexRecord, QueryError> {
    // A pending cleanup still owns the name; report that before validating
    // fields or the embedding provider, matching scalar index creation.
    cassie
        .midge
        .ensure_no_pending_index_cleanup(statement.table.as_ref(), &statement.name)?;
    let schema = collection_schema(cassie, &statement.table)?;
    let vector_field = vector_field(&schema, statement)?;
    let source_field = statement
        .options
        .get("source_field")
        .cloned()
        .unwrap_or_else(|| vector_field.name.clone());
    let direct_vector_index = source_field == vector_field.name;
    let dimensions = vector_dimensions(cassie, vector_field, !direct_vector_index)?;
    if !direct_vector_index {
        validate_source_field(&schema, &statement.table, &source_field)?;
    }

    let mut options = statement.options.clone();
    let normalized_options =
        normalize_vector_index_options(&mut options).map_err(QueryError::General)?;

    let metadata = VectorIndexMetadata {
        provider: cassie.embedding_provider.provider_name().to_string(),
        model: cassie.embedding_provider.model_name().to_string(),
        dimensions,
        metric: statement
            .options
            .get("metric")
            .and_then(|metric| metric.parse::<DistanceMetric>().ok())
            .unwrap_or(DistanceMetric::Cosine),
        index_type: normalized_options.index_type,
        hnsw: normalized_options.hnsw,
        hnsw_graph: None,
        ivfflat: normalized_options.ivfflat,
        ivfflat_training: None,
    };

    Ok(VectorIndexRecord {
        collection: statement.table.to_string(),
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
    validate_provider: bool,
) -> Result<usize, QueryError> {
    let DataType::Vector(Some(dimensions)) = vector_field.data_type else {
        return Err(QueryError::General(format!(
            "field '{}' is not a vector field",
            vector_field.name
        )));
    };
    if validate_provider && cassie.embedding_provider.dimensions() != dimensions {
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
