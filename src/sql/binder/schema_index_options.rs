use super::{CassieError, Catalog, CollectionSchema, DistanceMetric, Expr};
use crate::catalog::local_name;
use crate::sql::ast::{CreateIndexStatement, IdentifierPath};
use crate::types::DataType;
use crate::vector::index_options::normalize_vector_index_options;

/// The catalog and name-resolution scope a `CREATE INDEX` binds within.
pub(super) struct IndexBindingScope<'a> {
    pub(super) catalog: &'a Catalog,
    pub(super) database: &'a str,
    pub(super) search_path: &'a [String],
}

pub(super) fn bind_vector_index_options(
    statement: &mut CreateIndexStatement,
    scope: &IndexBindingScope<'_>,
    schema: &CollectionSchema,
    table: &str,
    name: &str,
    fields: &[String],
) -> Result<(), CassieError> {
    let catalog = scope.catalog;
    let field = &fields[0];
    validate_vector_index_field(catalog, schema, table, name, field)?;
    let access_method = statement
        .options
        .get(crate::catalog::IndexMeta::ACCESS_METHOD_OPTION)
        .map_or_else(|| "vector".to_owned(), Clone::clone);
    if matches!(access_method.as_str(), "hnsw" | "ivfflat") {
        return bind_pgvector_index_options(statement, scope, schema, name, field, &access_method);
    }
    if access_method != "vector" {
        return Err(CassieError::Planner(format!(
            "unsupported vector access method '{access_method}'"
        )));
    }
    let source_field = validate_vector_source_field(statement, schema, table)?.to_string();

    let metric = parse_vector_metric(statement.options.get("metric").map(String::as_str))?;
    statement
        .options
        .insert("metric".to_string(), metric.as_str().to_string());
    normalize_vector_index_options(&mut statement.options).map_err(CassieError::Planner)?;
    validate_vector_option_keys(statement, name, table)?;
    statement
        .options
        .insert("source_field".to_string(), source_field);
    Ok(())
}

fn bind_pgvector_index_options(
    statement: &mut CreateIndexStatement,
    scope: &IndexBindingScope<'_>,
    schema: &CollectionSchema,
    name: &str,
    field: &str,
    access_method: &str,
) -> Result<(), CassieError> {
    let search_path = scope.search_path;
    let extension = scope
        .catalog
        .get_extension(scope.database, "vector")
        .ok_or_else(|| {
            CassieError::Planner("pgvector index methods require the vector extension".into())
        })?;
    let extension_schema = local_name(&extension.schema);
    let field_entry = schema
        .fields
        .iter()
        .find(|entry| entry.name == field)
        .expect("validated vector index field exists");
    let crate::types::DataType::Vector(Some(dimensions)) = field_entry.data_type else {
        return Err(CassieError::Planner(format!(
            "pgvector {access_method} index '{name}' requires a fixed-dimension vector field"
        )));
    };
    if !(1..=2_000).contains(&dimensions) {
        return Err(CassieError::Planner(format!(
            "pgvector {access_method} indexes support vector dimensions from 1 through 2000"
        )));
    }

    let operator_class_key = crate::catalog::IndexMeta::key_option_name(0, "operator_class");
    let explicit_operator_class = statement.options.get(&operator_class_key).cloned();
    let operator_class = explicit_operator_class
        .as_deref()
        .or_else(|| (access_method == "ivfflat").then_some("vector_l2_ops"))
        .ok_or_else(|| {
            CassieError::Planner(format!(
                "pgvector {access_method} index requires an operator class"
            ))
        })?;
    let operator_class_name = if explicit_operator_class.is_some() {
        resolve_pgvector_operator_class(
            operator_class,
            &extension_schema,
            search_path,
            access_method,
        )?
    } else {
        "vector_l2_ops".to_string()
    };
    let metric = match operator_class_name.as_str() {
        "vector_l2_ops" => "l2",
        "vector_ip_ops" => "dot",
        "vector_cosine_ops" => "cosine",
        _ => {
            return Err(CassieError::Planner(format!(
                "operator class '{operator_class}' is not supported for pgvector {access_method}"
            )))
        }
    };
    if statement.options.contains_key("metric") || statement.options.contains_key("source_field") {
        return Err(CassieError::Planner(format!(
            "pgvector {access_method} indexes select their metric from the operator class and index the vector field directly"
        )));
    }
    for key in statement.options.keys() {
        let allowed = match access_method {
            "hnsw" => matches!(key.as_str(), "m" | "ef_construction"),
            "ivfflat" => key == "lists",
            _ => false,
        } || key == crate::catalog::IndexMeta::ACCESS_METHOD_OPTION
            || key == &operator_class_key;
        if !allowed {
            return Err(CassieError::Planner(format!(
                "unsupported pgvector {access_method} index option '{key}'"
            )));
        }
    }

    let key_option_names = ["order", "nulls"];
    if key_option_names.iter().any(|option| {
        statement
            .options
            .contains_key(&crate::catalog::IndexMeta::key_option_name(0, option))
    }) {
        return Err(CassieError::Planner(
            "pgvector indexes do not accept key ordering options".into(),
        ));
    }

    statement
        .options
        .insert("index_type".to_string(), access_method.to_string());
    statement
        .options
        .entry(operator_class_key)
        .or_insert_with(|| operator_class.to_string());
    statement
        .options
        .insert("metric".to_string(), metric.to_string());
    statement
        .options
        .insert("source_field".to_string(), field.to_string());
    normalize_vector_index_options(&mut statement.options).map_err(CassieError::Planner)?;
    Ok(())
}

fn resolve_pgvector_operator_class(
    raw_name: &str,
    extension_schema: &str,
    search_path: &[String],
    access_method: &str,
) -> Result<String, CassieError> {
    let path = IdentifierPath::parse(raw_name).map_err(CassieError::Planner)?;
    let components = path.components();
    let (schema_component, class_component) = match components {
        [class] => (None, class),
        [schema, class] => (Some(schema), class),
        _ => {
            return Err(CassieError::Planner(format!(
                "operator class '{raw_name}' does not resolve for pgvector {access_method}"
            )));
        }
    };
    let class_name = if class_component.is_delimited() {
        class_component.value().to_owned()
    } else {
        class_component.value().to_ascii_lowercase()
    };
    let schema_resolves = schema_component.map_or_else(
        || {
            search_path
                .iter()
                .any(|schema| local_name(schema).eq_ignore_ascii_case(extension_schema))
        },
        |schema| {
            if schema.is_delimited() {
                schema.value() == extension_schema
            } else {
                schema.value().eq_ignore_ascii_case(extension_schema)
            }
        },
    );
    if !schema_resolves {
        return Err(CassieError::Planner(format!(
            "operator class '{raw_name}' does not resolve in vector extension schema '{extension_schema}' for pgvector {access_method}"
        )));
    }
    Ok(class_name)
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
    let mut parts = bucket_width.split_whitespace();
    let amount = parts.next().and_then(|value| value.parse::<u64>().ok());
    let unit_seconds = match parts.next().map(str::to_ascii_lowercase).as_deref() {
        Some("minute" | "minutes") => Some(60_u64),
        Some("hour" | "hours") => Some(60 * 60),
        Some("day" | "days") => Some(24 * 60 * 60),
        _ => None,
    };
    let valid = amount
        .zip(unit_seconds)
        .and_then(|(amount, unit_seconds)| amount.checked_mul(unit_seconds))
        .is_some_and(|seconds| seconds > 0 && i64::try_from(seconds).is_ok())
        && parts.next().is_none();
    if !valid {
        return Err(CassieError::Planner(
            "time-series index bucket_width must be a positive minute, hour, or day interval"
                .into(),
        ));
    }
    Ok(bucket_width)
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
