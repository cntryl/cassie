use super::{CassieError, Catalog, CollectionSchema, DataType, DistanceMetric, Expr};
use crate::sql::ast::CreateIndexStatement;

pub(super) fn bind_vector_index_options(
    statement: &mut CreateIndexStatement,
    catalog: &Catalog,
    schema: &CollectionSchema,
    table: &str,
    name: &str,
    fields: &[String],
) -> Result<(), CassieError> {
    let field = &fields[0];
    validate_vector_index_field(catalog, schema, table, name, field)?;
    let source_field = validate_vector_source_field(statement, schema, table)?.to_string();

    let metric = parse_vector_metric(statement.options.get("metric").map(String::as_str))?;
    statement
        .options
        .insert("metric".to_string(), metric.as_str().to_string());
    let index_type = normalize_vector_index_type(statement)?;
    apply_vector_index_type_options(statement, &index_type)?;
    validate_vector_option_keys(statement, name, table)?;
    statement
        .options
        .insert("source_field".to_string(), source_field);
    Ok(())
}

pub(super) fn bind_time_series_index_options(
    statement: &mut CreateIndexStatement,
    schema: &CollectionSchema,
    table: &str,
    name: &str,
    fields: &[String],
    expressions: &[Expr],
    include_fields: &[String],
) -> Result<(), CassieError> {
    validate_time_series_index_shape(statement, fields, expressions, include_fields)?;
    let timestamp_field = fields.first().expect("validated field exists");
    validate_time_series_timestamp_field(schema, table, name, timestamp_field)?;
    let bucket_width = parse_time_series_bucket_width(statement)?;
    let partition_by = parse_time_series_partition_by(statement);
    validate_time_series_partition_fields(schema, table, &partition_by)?;
    validate_time_series_option_keys(statement, name, table)?;
    statement
        .options
        .insert("bucket_width".to_string(), bucket_width);
    if !partition_by.is_empty() {
        statement
            .options
            .insert("partition_by".to_string(), partition_by.join(","));
    }
    Ok(())
}

fn validate_vector_index_field(
    catalog: &Catalog,
    schema: &CollectionSchema,
    table: &str,
    name: &str,
    field: &str,
) -> Result<(), CassieError> {
    let field_entry = schema
        .fields
        .iter()
        .find(|entry| entry.name == field)
        .ok_or_else(|| {
            CassieError::Planner(format!(
                "index field '{field}' does not exist on collection '{table}'"
            ))
        })?;

    if let Some(existing_vector) = catalog.get_vector_index(table, field) {
        let existing_index = catalog
            .get_index(table, name)
            .filter(|metadata| metadata.field == existing_vector.field)
            .filter(|metadata| metadata.kind == crate::catalog::IndexKind::Vector);

        if existing_index.is_none() {
            return Err(CassieError::Planner(format!(
                "vector index on field '{}' already exists on collection '{}'",
                existing_vector.field, table
            )));
        }
    }

    if matches!(field_entry.data_type, DataType::Vector(_)) {
        Ok(())
    } else {
        Err(CassieError::Planner(format!(
            "vector index '{name}' requires vector field '{field}'"
        )))
    }
}

fn validate_vector_source_field<'a>(
    statement: &'a CreateIndexStatement,
    schema: &CollectionSchema,
    table: &str,
) -> Result<&'a str, CassieError> {
    let source_field = statement
        .options
        .get("source_field")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CassieError::Planner("CREATE INDEX USING vector requires source_field".into())
        })?;

    let source_entry = schema
        .fields
        .iter()
        .find(|entry| entry.name == source_field)
        .ok_or_else(|| {
            CassieError::Planner(format!(
                "source field '{source_field}' does not exist on collection '{table}'"
            ))
        })?;

    if matches!(source_entry.data_type, DataType::Text | DataType::Json) {
        Ok(source_field)
    } else {
        Err(CassieError::Planner(format!(
            "source field '{source_field}' must be text/json for vector index"
        )))
    }
}

fn normalize_vector_index_type(
    statement: &mut CreateIndexStatement,
) -> Result<String, CassieError> {
    let index_type = statement
        .options
        .get("index_type")
        .map_or("bruteforce", String::as_str)
        .trim()
        .to_ascii_lowercase();
    if !matches!(index_type.as_str(), "bruteforce" | "hnsw" | "ivfflat") {
        return Err(CassieError::Planner(format!(
            "unsupported vector index_type '{index_type}'"
        )));
    }
    statement
        .options
        .insert("index_type".to_string(), index_type.clone());
    Ok(index_type)
}

fn apply_vector_index_type_options(
    statement: &mut CreateIndexStatement,
    index_type: &str,
) -> Result<(), CassieError> {
    match index_type {
        "hnsw" => apply_hnsw_options(statement),
        "ivfflat" => apply_ivfflat_options(statement),
        "bruteforce" => Ok(()),
        _ => unreachable!(),
    }
}

fn apply_hnsw_options(statement: &mut CreateIndexStatement) -> Result<(), CassieError> {
    let m = parse_vector_index_usize_option(statement.options.get("m"), "m", 16, 2, 128)?;
    let ef_construction = parse_vector_index_usize_option(
        statement.options.get("ef_construction"),
        "ef_construction",
        64,
        m,
        4096,
    )?;
    let ef_search = parse_vector_index_usize_option(
        statement.options.get("ef_search"),
        "ef_search",
        40,
        1,
        4096,
    )?;
    statement.options.insert("m".to_string(), m.to_string());
    statement
        .options
        .insert("ef_construction".to_string(), ef_construction.to_string());
    statement
        .options
        .insert("ef_search".to_string(), ef_search.to_string());
    Ok(())
}

fn apply_ivfflat_options(statement: &mut CreateIndexStatement) -> Result<(), CassieError> {
    let lists =
        parse_vector_index_usize_option(statement.options.get("lists"), "lists", 64, 1, 65_536)?;
    let probes =
        parse_vector_index_usize_option(statement.options.get("probes"), "probes", 1, 1, lists)?;
    let training_sample_size = parse_vector_index_usize_option(
        statement.options.get("training_sample_size"),
        "training_sample_size",
        lists.saturating_mul(40).max(1),
        lists,
        10_000_000,
    )?;
    let training_seed = parse_vector_index_usize_option(
        statement.options.get("training_seed"),
        "training_seed",
        1,
        0,
        usize::MAX,
    )?;
    statement
        .options
        .insert("lists".to_string(), lists.to_string());
    statement
        .options
        .insert("probes".to_string(), probes.to_string());
    statement.options.insert(
        "training_sample_size".to_string(),
        training_sample_size.to_string(),
    );
    statement
        .options
        .insert("training_seed".to_string(), training_seed.to_string());
    Ok(())
}

fn validate_vector_option_keys(
    statement: &CreateIndexStatement,
    name: &str,
    table: &str,
) -> Result<(), CassieError> {
    for key in statement.options.keys() {
        if !matches!(
            key.as_str(),
            "source_field"
                | "metric"
                | "index_type"
                | "m"
                | "ef_construction"
                | "ef_search"
                | "lists"
                | "probes"
                | "training_sample_size"
                | "training_seed"
        ) {
            return Err(CassieError::Planner(format!(
                "unsupported vector index option '{key}' for '{name}' on collection '{table}'"
            )));
        }
    }
    Ok(())
}

fn validate_time_series_index_shape(
    statement: &CreateIndexStatement,
    fields: &[String],
    expressions: &[Expr],
    include_fields: &[String],
) -> Result<(), CassieError> {
    if fields.len() != 1 {
        return Err(CassieError::Planner(
            "time-series index requires exactly one timestamp field".into(),
        ));
    }
    if !expressions.is_empty() {
        return Err(CassieError::Planner(
            "time-series indexes do not support expressions".into(),
        ));
    }
    if statement.unique {
        return Err(CassieError::Planner(
            "time-series indexes cannot be unique".into(),
        ));
    }
    if !include_fields.is_empty() {
        return Err(CassieError::Planner(
            "time-series indexes do not support INCLUDE columns".into(),
        ));
    }
    if statement.predicate.is_some() {
        return Err(CassieError::Planner(
            "partial time-series indexes are not supported".into(),
        ));
    }
    Ok(())
}

fn validate_time_series_timestamp_field(
    schema: &CollectionSchema,
    table: &str,
    name: &str,
    timestamp_field: &str,
) -> Result<(), CassieError> {
    let field_entry = schema
        .fields
        .iter()
        .find(|entry| entry.name == timestamp_field)
        .ok_or_else(|| {
            CassieError::Planner(format!(
                "time-series index field '{timestamp_field}' does not exist on collection '{table}'"
            ))
        })?;
    if matches!(field_entry.data_type, DataType::Timestamp) {
        Ok(())
    } else {
        Err(CassieError::Planner(format!(
            "time-series index '{name}' requires timestamp field '{timestamp_field}'"
        )))
    }
}

fn parse_time_series_bucket_width(statement: &CreateIndexStatement) -> Result<String, CassieError> {
    let bucket_width = statement
        .options
        .get("bucket_width")
        .map_or("1 hour", String::as_str)
        .trim()
        .to_string();
    if bucket_width.is_empty() {
        Err(CassieError::Planner(
            "time-series index option 'bucket_width' cannot be empty".into(),
        ))
    } else {
        Ok(bucket_width)
    }
}

fn parse_time_series_partition_by(statement: &CreateIndexStatement) -> Vec<String> {
    statement
        .options
        .get("partition_by")
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|field| !field.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn validate_time_series_partition_fields(
    schema: &CollectionSchema,
    table: &str,
    partition_by: &[String],
) -> Result<(), CassieError> {
    for partition_field in partition_by {
        if !schema
            .fields
            .iter()
            .any(|entry| entry.name == *partition_field)
        {
            return Err(CassieError::Planner(format!(
                "time-series partition field '{partition_field}' does not exist on collection '{table}'"
            )));
        }
    }
    Ok(())
}

fn validate_time_series_option_keys(
    statement: &CreateIndexStatement,
    name: &str,
    table: &str,
) -> Result<(), CassieError> {
    for key in statement.options.keys() {
        if !matches!(key.as_str(), "bucket_width" | "partition_by") {
            return Err(CassieError::Planner(format!(
                "unsupported time-series index option '{key}' for '{name}' on collection '{table}'"
            )));
        }
    }
    Ok(())
}

fn parse_vector_metric(raw_metric: Option<&str>) -> Result<DistanceMetric, CassieError> {
    let metric = raw_metric.unwrap_or("cosine");
    metric.parse().map_err(|()| {
        CassieError::Planner(format!(
            "unsupported vector metric '{metric}' (expected cosine, l2, or dot)"
        ))
    })
}

fn parse_vector_index_usize_option(
    value: Option<&String>,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, CassieError> {
    let value = value.map_or("", String::as_str).trim();
    if value.is_empty() {
        return Ok(default);
    }
    let parsed = value
        .parse::<usize>()
        .map_err(|_| CassieError::Planner(format!("invalid vector index option '{key}'")))?;
    if parsed < min || parsed > max {
        return Err(CassieError::Planner(format!(
            "vector index option '{key}' must be in [{min}, {max}]"
        )));
    }
    Ok(parsed)
}
