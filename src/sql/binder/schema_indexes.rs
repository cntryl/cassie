use super::{
    bm25, normalize_relation_name, resolve_relation_name, schema_index_options, BindingContext,
    CassieError, Catalog, CollectionSchema, DataType, Expr, HashSet,
};
use crate::catalog::{derive_scoped_name, parse_name, ParsedName};
use crate::sql::ast::CreateIndexStatement;

pub(super) fn bind_create_index(
    mut statement: CreateIndexStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CreateIndexStatement, CassieError> {
    let table = resolve_relation_name(statement.table.trim(), catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires a collection name".into(),
        ));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let name = match parse_name(statement.name.trim()).map_err(CassieError::Planner)? {
        ParsedName::Unqualified(name) => derive_scoped_name(&table, |_| name),
        _ => normalize_relation_name(statement.name.trim(), context)?,
    };
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires an index name".into(),
        ));
    }

    let fields = normalize_fields(&statement.fields);
    let expressions = statement.expressions.clone();
    if fields.is_empty() && expressions.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires at least one indexed field".into(),
        ));
    }

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;
    validate_index_shape(&statement, &fields, &expressions)?;

    let include_fields = normalize_fields(&statement.include_fields);
    validate_include_fields(&statement, &schema, &table, &fields, &include_fields)?;
    validate_index_fields_and_expressions(&schema, &table, &fields, &expressions)?;

    match statement.kind {
        crate::catalog::IndexKind::Vector => schema_index_options::bind_vector_index_options(
            &mut statement,
            catalog,
            &schema,
            &table,
            &name,
            fields.as_slice(),
        )?,
        crate::catalog::IndexKind::FullText => {
            bind_fulltext_index_options(&mut statement, catalog, &schema, &table, &name, &fields)?;
        }
        crate::catalog::IndexKind::Column => {
            bind_column_index_options(
                &mut statement,
                &table,
                &name,
                &fields,
                &expressions,
                &include_fields,
            )?;
        }
        crate::catalog::IndexKind::TimeSeries => {
            schema_index_options::bind_time_series_index_options(
                &mut statement,
                &schema,
                &table,
                &name,
                fields.as_slice(),
                expressions.as_slice(),
                include_fields.as_slice(),
            )?;
        }
        crate::catalog::IndexKind::Scalar | crate::catalog::IndexKind::Hybrid => {}
    }

    if !statement.if_not_exists && catalog.get_index(&table, &name).is_some() {
        return Err(CassieError::Planner(format!(
            "index '{name}' already exists on collection '{table}'"
        )));
    }

    statement.table = table;
    statement.name = name;
    statement.fields = fields;
    statement.expressions = expressions;
    statement.include_fields = include_fields;
    Ok(statement)
}

pub(super) fn validate_index_expression(
    expr: &Expr,
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => {
            if known_fields.contains(name) {
                Ok(())
            } else {
                Err(CassieError::Planner(format!(
                    "index expression references unknown field '{name}'"
                )))
            }
        }
        Expr::Param(_) => Err(CassieError::Planner(
            "index expressions cannot reference query parameters".into(),
        )),
        Expr::Exists(_) => Err(CassieError::Planner(
            "index expressions cannot contain subqueries".into(),
        )),
        Expr::Function(function) => validate_index_expression_function(function, known_fields),
        Expr::Binary { left, right, .. } => {
            validate_index_expression(left, known_fields)?;
            validate_index_expression(right, known_fields)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            validate_index_expression(expr, known_fields)
        }
        Expr::InList { expr, values, .. } => {
            validate_index_expression(expr, known_fields)?;
            for value in values {
                validate_index_expression(value, known_fields)?;
            }
            Ok(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            validate_index_expression(expr, known_fields)?;
            validate_index_expression(low, known_fields)?;
            validate_index_expression(high, known_fields)
        }
        Expr::StringLiteral(_) | Expr::NumberLiteral(_) | Expr::BoolLiteral(_) | Expr::Null => {
            Ok(())
        }
    }
}

pub(super) fn parse_column_index_segment_size(
    value: Option<&String>,
) -> Result<usize, CassieError> {
    let value = value.map_or("", String::as_str).trim();
    if value.is_empty() {
        return Ok(1024);
    }
    let parsed = value
        .parse::<usize>()
        .map_err(|_| CassieError::Planner("invalid column index option 'segment_size'".into()))?;
    if !(1..=1_000_000).contains(&parsed) {
        return Err(CassieError::Planner(
            "column index option 'segment_size' must be in [1, 1000000]".into(),
        ));
    }
    Ok(parsed)
}

pub(super) fn parse_fulltext_index_float_option(
    key: &str,
    value: Option<&str>,
    default: f64,
    min: f64,
    max: Option<f64>,
) -> Result<f64, CassieError> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        return Ok(default);
    }

    let parsed = value
        .parse::<f64>()
        .map_err(|_| CassieError::Planner(format!("invalid {key} value '{value}'")))?;

    if !parsed.is_finite() {
        return Err(CassieError::Planner(format!(
            "fulltext index option '{key}' must be finite"
        )));
    }

    let range_ok = max.map_or(parsed >= min, |max| parsed >= min && parsed <= max);
    if !range_ok {
        return match max {
            Some(max) => Err(CassieError::Planner(format!(
                "fulltext index option '{key}' must be in [{min}, {max}]"
            ))),
            None => Err(CassieError::Planner(format!(
                "fulltext index option '{key}' must be at least {min}"
            ))),
        };
    }

    Ok(parsed)
}

fn normalize_fields(fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .map(|field| field.trim().to_string())
        .filter(|field| !field.is_empty())
        .collect()
}

fn validate_index_shape(
    statement: &CreateIndexStatement,
    fields: &[String],
    expressions: &[Expr],
) -> Result<(), CassieError> {
    if !matches!(
        statement.kind,
        crate::catalog::IndexKind::Scalar | crate::catalog::IndexKind::Column
    ) && fields.len() + expressions.len() > 1
    {
        return Err(CassieError::Planner(
            "composite indexes are only supported for scalar index methods".into(),
        ));
    }
    if !expressions.is_empty() && !matches!(statement.kind, crate::catalog::IndexKind::Scalar) {
        return Err(CassieError::Planner(
            "expression indexes are only supported for scalar indexes".into(),
        ));
    }
    if !statement.include_fields.is_empty()
        && !matches!(statement.kind, crate::catalog::IndexKind::Scalar)
    {
        return Err(CassieError::Planner(
            "INCLUDE columns are only supported for scalar indexes".into(),
        ));
    }
    Ok(())
}

fn validate_include_fields(
    statement: &CreateIndexStatement,
    schema: &CollectionSchema,
    table: &str,
    fields: &[String],
    include_fields: &[String],
) -> Result<(), CassieError> {
    if include_fields.is_empty() {
        return Ok(());
    }
    if !matches!(statement.kind, crate::catalog::IndexKind::Scalar) {
        return Err(CassieError::Planner(
            "INCLUDE columns are only supported for scalar indexes".into(),
        ));
    }

    let mut seen_include_fields = std::collections::BTreeSet::new();
    let key_fields = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    for field in include_fields {
        let normalized = field.to_ascii_lowercase();
        if !seen_include_fields.insert(normalized.clone()) {
            return Err(CassieError::Planner(format!(
                "INCLUDE field '{field}' is duplicated"
            )));
        }
        if key_fields.contains(&normalized) {
            return Err(CassieError::Planner(format!(
                "INCLUDE field '{field}' duplicates an index key field"
            )));
        }
        if !schema.fields.iter().any(|entry| entry.name == *field) {
            return Err(CassieError::Planner(format!(
                "INCLUDE field '{field}' does not exist on collection '{table}'"
            )));
        }
    }
    Ok(())
}

fn validate_index_fields_and_expressions(
    schema: &CollectionSchema,
    table: &str,
    fields: &[String],
    expressions: &[Expr],
) -> Result<(), CassieError> {
    for field in fields {
        if !schema.fields.iter().any(|entry| entry.name == *field) {
            return Err(CassieError::Planner(format!(
                "index field '{field}' does not exist on collection '{table}'"
            )));
        }
    }

    let known_fields = schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();
    for expression in expressions {
        validate_index_expression(expression, &known_fields)?;
    }
    Ok(())
}

fn bind_fulltext_index_options(
    statement: &mut CreateIndexStatement,
    catalog: &Catalog,
    schema: &CollectionSchema,
    table: &str,
    name: &str,
    fields: &[String],
) -> Result<(), CassieError> {
    let field = &fields[0];
    let field_entry = schema
        .fields
        .iter()
        .find(|entry| entry.name == *field)
        .ok_or_else(|| {
            CassieError::Planner(format!(
                "index field '{field}' does not exist on collection '{table}'"
            ))
        })?;

    if !matches!(field_entry.data_type, DataType::Text) {
        return Err(CassieError::Planner(format!(
            "fulltext index '{name}' requires text field '{field}'"
        )));
    }

    validate_fulltext_uniqueness(catalog, table, name, field)?;
    validate_fulltext_index_option_keys(statement, table, name)?;

    let boost = parse_fulltext_index_float_option(
        "boost",
        statement.options.get("boost").map(String::as_str),
        bm25::DEFAULT_FULLTEXT_BOOST,
        0.0,
        None,
    )?;
    let k1 = parse_fulltext_index_float_option(
        "k1",
        statement.options.get("k1").map(String::as_str),
        bm25::DEFAULT_BM25_K1,
        0.0,
        None,
    )?;
    let b = parse_fulltext_index_float_option(
        "b",
        statement.options.get("b").map(String::as_str),
        bm25::DEFAULT_BM25_B,
        0.0,
        Some(1.0),
    )?;
    let analyzer = crate::search::analyzer::AnalyzerConfig::from_index_options(&statement.options)
        .map_err(CassieError::Planner)?;

    statement
        .options
        .insert("boost".to_string(), boost.to_string());
    statement.options.insert("k1".to_string(), k1.to_string());
    statement.options.insert("b".to_string(), b.to_string());
    statement
        .options
        .insert("analyzer".to_string(), analyzer.name);
    statement
        .options
        .insert("tokenizer".to_string(), analyzer.tokenizer);
    statement.options.insert(
        "case_folding".to_string(),
        analyzer.case_folding.to_string(),
    );
    statement
        .options
        .insert("stop_words".to_string(), analyzer.stop_words);
    statement
        .options
        .insert("stemming".to_string(), analyzer.stemming);
    statement.options.insert(
        "accent_folding".to_string(),
        analyzer.accent_folding.to_string(),
    );
    Ok(())
}

fn validate_fulltext_uniqueness(
    catalog: &Catalog,
    table: &str,
    name: &str,
    field: &str,
) -> Result<(), CassieError> {
    let existing_fulltext_index = catalog.list_indexes(table).into_iter().find(|metadata| {
        metadata.kind == crate::catalog::IndexKind::FullText
            && metadata.field.eq_ignore_ascii_case(field)
    });
    if let Some(existing_fulltext_index) = existing_fulltext_index {
        let existing_index = catalog
            .get_index(table, name)
            .filter(|metadata| metadata.kind == crate::catalog::IndexKind::FullText)
            .filter(|metadata| {
                metadata
                    .field
                    .eq_ignore_ascii_case(&existing_fulltext_index.field)
            });

        if existing_index.is_none() {
            return Err(CassieError::Planner(format!(
                "fulltext index on field '{field}' already exists on collection '{table}'"
            )));
        }
    }
    Ok(())
}

fn validate_fulltext_index_option_keys(
    statement: &CreateIndexStatement,
    table: &str,
    name: &str,
) -> Result<(), CassieError> {
    for key in statement.options.keys() {
        if !matches!(
            key.as_str(),
            "boost"
                | "k1"
                | "b"
                | "analyzer"
                | "case_folding"
                | "stop_words"
                | "stemming"
                | "accent_folding"
                | "tokenizer"
        ) {
            return Err(CassieError::Planner(format!(
                "unsupported fulltext index option '{key}' for '{name}' on collection '{table}'"
            )));
        }
    }
    Ok(())
}

fn bind_column_index_options(
    statement: &mut CreateIndexStatement,
    table: &str,
    name: &str,
    fields: &[String],
    expressions: &[Expr],
    include_fields: &[String],
) -> Result<(), CassieError> {
    if fields.is_empty() {
        return Err(CassieError::Planner(
            "column index requires at least one field".into(),
        ));
    }
    if !expressions.is_empty() {
        return Err(CassieError::Planner(
            "column indexes do not support expressions".into(),
        ));
    }
    if statement.unique {
        return Err(CassieError::Planner(
            "column indexes cannot be unique".into(),
        ));
    }
    if !include_fields.is_empty() {
        return Err(CassieError::Planner(
            "column indexes do not support INCLUDE columns".into(),
        ));
    }
    if statement.predicate.is_some() {
        return Err(CassieError::Planner(
            "partial column indexes are not supported".into(),
        ));
    }

    let mut seen_fields = std::collections::BTreeSet::new();
    for field in fields {
        if !seen_fields.insert(field.to_ascii_lowercase()) {
            return Err(CassieError::Planner(format!(
                "column index field '{field}' is duplicated"
            )));
        }
    }

    let segment_size = parse_column_index_segment_size(statement.options.get("segment_size"))?;
    for key in statement.options.keys() {
        if key != "segment_size" {
            return Err(CassieError::Planner(format!(
                "unsupported column index option '{key}' for '{name}' on collection '{table}'"
            )));
        }
    }
    statement
        .options
        .insert("segment_size".to_string(), segment_size.to_string());
    Ok(())
}

fn validate_index_expression_function(
    function: &crate::sql::ast::FunctionCall,
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let normalized = function.name.to_ascii_lowercase();
    if crate::sql::functions::is_aggregate_function(&normalized) {
        return Err(CassieError::Planner(format!(
            "aggregate function '{}' is not allowed in index expressions",
            function.name
        )));
    }
    let Some(definition) = crate::sql::functions::registry()
        .into_iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(&normalized))
    else {
        return Err(CassieError::Planner(format!(
            "function '{}' is not allowed in index expressions",
            function.name
        )));
    };
    if !matches!(
        definition.name,
        "length" | "lower" | "upper" | "substring" | "trim" | "concat" | "coalesce" | "abs"
    ) {
        return Err(CassieError::Planner(format!(
            "function '{}' is not immutable for index expressions",
            function.name
        )));
    }
    if !definition.arity.matches(function.args.len()) {
        return Err(CassieError::Planner(format!(
            "function '{}' expects {}, got {}",
            function.name,
            definition.arity.describe(),
            function.args.len()
        )));
    }
    for arg in &function.args {
        validate_index_expression(arg, known_fields)?;
    }
    Ok(())
}
