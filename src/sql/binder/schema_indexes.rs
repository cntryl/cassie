use super::{
    bm25, normalize_relation_name, resolve_relation_path, schema_index_options, BindingContext,
    CassieError, Catalog, CollectionSchema, DataType, Expr, HashSet,
};
use crate::catalog::{derive_scoped_name, local_name, parse_name, IndexMeta, ParsedName};
use crate::sql::ast::{CreateIndexStatement, IdentifierPath};

const MAX_GENERATED_INDEX_IDENTIFIER_BYTES: usize = 63;

pub(super) fn bind_create_index(
    mut statement: CreateIndexStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CreateIndexStatement, CassieError> {
    let table = resolve_relation_path(&statement.table, catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires a collection name".into(),
        ));
    }
    let is_sql_materialized_view = catalog.is_sql_materialized_view(&table);
    if !catalog.exists(&table) && !is_sql_materialized_view {
        return Err(CassieError::CollectionNotFound(table));
    }
    reject_unsupported_scalar_access_method(&statement)?;
    if is_sql_materialized_view && statement.kind != crate::catalog::IndexKind::Scalar {
        return Err(CassieError::Unsupported(
            "only scalar btree indexes are supported on SQL materialized views".into(),
        ));
    }

    let mut name = if statement.name.trim().is_empty() {
        String::new()
    } else {
        match parse_name(statement.name.trim()).map_err(CassieError::Planner)? {
            ParsedName::Unqualified(name) => derive_scoped_name(&table, |_| name),
            _ => normalize_relation_name(statement.name.trim(), context)?,
        }
    };
    if !name.is_empty() && statement.if_not_exists && catalog.get_index(&table, &name).is_some() {
        statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
        statement.name = name;
        return Ok(statement);
    }

    for expr in statement.expressions.iter().chain(&statement.predicate) {
        crate::sql::definition_guard::expression(expr)?;
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
    let fields = declared_field_names(&schema, fields);
    let expressions = declared_index_expressions(&schema, &expressions);
    if name.is_empty() {
        name = generated_index_name(&table, &fields, catalog);
    }
    if statement.if_not_exists && catalog.get_index(&table, &name).is_some() {
        statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
        statement.name = name;
        return Ok(statement);
    }
    validate_index_shape(&statement, &fields, &expressions)?;

    let include_fields = declared_field_names(&schema, normalize_fields(&statement.include_fields));
    validate_include_fields(&statement, &schema, &table, &fields, &include_fields)?;
    validate_index_fields_and_expressions(&schema, &table, &fields, &expressions)?;
    bind_index_method_options(
        &mut statement,
        IndexMethodBinding {
            catalog,
            schema: &schema,
            table: &table,
            name: &name,
            fields: &fields,
            expressions: &expressions,
            include_fields: &include_fields,
        },
    )?;

    if let Some(predicate) = &mut statement.predicate {
        bind_index_predicate(predicate, &schema, &table, catalog, context)?;
    }

    if !statement.if_not_exists && catalog.get_index(&table, &name).is_some() {
        return Err(CassieError::Planner(format!(
            "index '{name}' already exists on collection '{table}'"
        )));
    }

    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    statement.name = name;
    statement.fields = fields;
    statement.expressions = expressions;
    statement.include_fields = include_fields;
    Ok(statement)
}

#[derive(Clone, Copy)]
struct IndexMethodBinding<'a> {
    catalog: &'a Catalog,
    schema: &'a CollectionSchema,
    table: &'a str,
    name: &'a str,
    fields: &'a [String],
    expressions: &'a [Expr],
    include_fields: &'a [String],
}

fn bind_index_method_options(
    statement: &mut CreateIndexStatement,
    binding: IndexMethodBinding<'_>,
) -> Result<(), CassieError> {
    let IndexMethodBinding {
        catalog,
        schema,
        table,
        name,
        fields,
        expressions,
        include_fields,
    } = binding;
    validate_scalar_index_key_options(statement, schema, fields, expressions)?;
    validate_scalar_storage_options(statement)?;
    match statement.kind {
        crate::catalog::IndexKind::Vector => schema_index_options::bind_vector_index_options(
            statement, catalog, schema, table, name, fields,
        )?,
        crate::catalog::IndexKind::FullText => {
            bind_fulltext_index_options(statement, catalog, schema, table, name, fields)?;
        }
        crate::catalog::IndexKind::Column => {
            bind_column_index_options(statement, table, name, fields, expressions, include_fields)?;
        }
        crate::catalog::IndexKind::TimeSeries => {
            schema_index_options::bind_time_series_index_options(
                statement,
                schema,
                table,
                name,
                fields,
                expressions,
                include_fields,
            )?;
        }
        crate::catalog::IndexKind::Scalar | crate::catalog::IndexKind::Hybrid => {}
    }
    Ok(())
}

pub(super) fn validate_scalar_storage_options(
    statement: &mut CreateIndexStatement,
) -> Result<(), CassieError> {
    if statement.kind != crate::catalog::IndexKind::Scalar {
        return Ok(());
    }
    let storage_options = statement
        .options
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                IndexMeta::ACCESS_METHOD_OPTION | IndexMeta::NULLS_NOT_DISTINCT_OPTION
            )
        })
        .filter(|key| !key.starts_with("__cassie_index_key_"))
        .cloned()
        .collect::<Vec<_>>();
    if storage_options.is_empty() {
        return Ok(());
    }
    let access_method = statement
        .options
        .get(IndexMeta::ACCESS_METHOD_OPTION)
        .map_or("btree", String::as_str);
    if statement.kind != crate::catalog::IndexKind::Scalar
        || !access_method.eq_ignore_ascii_case("btree")
    {
        return Err(CassieError::Unsupported(
            "storage parameters are supported only for scalar btree indexes".into(),
        ));
    }
    for option in storage_options {
        let value = statement
            .options
            .get(&option)
            .expect("storage option key was collected from the map")
            .clone();
        let normalized = match option.as_str() {
            "fillfactor" => {
                let fillfactor = value
                    .parse::<u8>()
                    .ok()
                    .filter(|value| (10..=100).contains(value));
                fillfactor.map(|value| value.to_string()).ok_or_else(|| {
                    CassieError::Planner("btree fillfactor must be between 10 and 100".into())
                })?
            }
            "deduplicate_items" => normalize_storage_boolean(&option, &value)?,
            _ => {
                return Err(CassieError::Unsupported(format!(
                    "unsupported btree storage parameter '{option}'"
                )));
            }
        };
        statement.options.insert(option, normalized);
    }
    Ok(())
}

pub(super) fn normalize_storage_boolean(name: &str, value: &str) -> Result<String, CassieError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok("on".to_string()),
        "off" | "false" | "no" | "0" => Ok("off".to_string()),
        _ => Err(CassieError::Planner(format!(
            "btree {name} must be on or off"
        ))),
    }
}

fn reject_unsupported_scalar_access_method(
    statement: &CreateIndexStatement,
) -> Result<(), CassieError> {
    let hash_method = statement
        .options
        .get(IndexMeta::ACCESS_METHOD_OPTION)
        .is_some_and(|method| method.eq_ignore_ascii_case("hash"));
    if statement.kind == crate::catalog::IndexKind::Scalar && hash_method {
        return Err(CassieError::Unsupported(
            "hash indexes are not supported by the scalar index implementation".into(),
        ));
    }
    Ok(())
}

fn declared_index_expressions(schema: &CollectionSchema, expressions: &[Expr]) -> Vec<Expr> {
    expressions
        .iter()
        .map(|expression| declared_expression_columns(schema, expression))
        .collect()
}

fn generated_index_name(table: &str, fields: &[String], catalog: &Catalog) -> String {
    let indexed_columns = if fields.is_empty() {
        vec!["expression".to_string()]
    } else {
        fields.to_vec()
    };
    let base = format!("{}_{}_idx", local_name(table), indexed_columns.join("_"));
    let mut collision = 0_usize;

    loop {
        let suffix = if collision == 0 {
            String::new()
        } else {
            format!("_{collision}")
        };
        let prefix_limit = MAX_GENERATED_INDEX_IDENTIFIER_BYTES.saturating_sub(suffix.len());
        let local = format!(
            "{}{suffix}",
            truncate_identifier_prefix(&base, prefix_limit)
        );
        let scoped = derive_scoped_name(table, |_| local);
        if catalog.get_index(table, &scoped).is_none() {
            return scoped;
        }
        collision = collision.saturating_add(1);
    }
}

fn truncate_identifier_prefix(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(super) fn resolve_drop_index_name(
    raw: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<Option<IndexMeta>, CassieError> {
    let reference = parse_name(raw).map_err(CassieError::Planner)?;
    if matches!(reference, ParsedName::DatabaseQualified { .. }) {
        // Keep DROP INDEX inside the same database-scoping contract as relation
        // references, even when IF EXISTS is present.
        normalize_relation_name(raw, context)?;
    }

    let (database, schema, local) = match reference {
        ParsedName::Unqualified(local) => (None, None, local),
        ParsedName::SchemaQualified { schema, name } => (None, Some(schema), name),
        ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        } => (Some(database), Some(schema), name),
    };
    let indexes = catalog.all_indexes_snapshot();
    let schemas = schema.map_or_else(
        || {
            context
                .relation_search_order()
                .into_iter()
                .map(str::to_owned)
                .collect()
        },
        |schema| vec![schema],
    );

    for schema in schemas {
        let matching = indexes
            .iter()
            .filter(|index| {
                index_name_matches_scope(index, context, database.as_deref(), &schema, &local)
            })
            .cloned()
            .collect::<Vec<_>>();
        match matching.as_slice() {
            [] => {}
            [index] => return Ok(Some(index.clone())),
            _ => {
                return Err(CassieError::Planner(format!(
                    "index reference '{raw}' is ambiguous in schema '{schema}'"
                )));
            }
        }
    }
    Ok(None)
}

fn index_name_matches_scope(
    index: &IndexMeta,
    context: &BindingContext,
    database: Option<&str>,
    schema: &str,
    local: &str,
) -> bool {
    let (index_database, index_schema, index_local) = match parse_name(&index.name) {
        Ok(ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        }) => (Some(database), schema, name),
        Ok(ParsedName::SchemaQualified { schema, name }) => (
            crate::catalog::relation_database_name(&index.collection),
            schema,
            name,
        ),
        Ok(ParsedName::Unqualified(name)) => match parse_name(&index.collection) {
            Ok(ParsedName::DatabaseQualified {
                database,
                schema,
                name: _,
            }) => (Some(database), schema, name),
            Ok(ParsedName::SchemaQualified { schema, name: _ }) => (None, schema, name),
            Ok(ParsedName::Unqualified(_)) | Err(_) => {
                (None, crate::catalog::DEFAULT_SCHEMA.to_string(), name)
            }
        },
        Err(_) => return false,
    };
    let effective_database = index_database.unwrap_or_else(|| context.database.clone());
    let database_matches = database.map_or_else(
        || {
            !context.scopes_database_objects()
                || effective_database.eq_ignore_ascii_case(&context.database)
        },
        |database| effective_database.eq_ignore_ascii_case(database),
    );

    database_matches
        && index_schema.eq_ignore_ascii_case(schema)
        && index_local.eq_ignore_ascii_case(local)
}

fn bind_index_predicate(
    predicate: &mut Expr,
    schema: &CollectionSchema,
    table: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    *predicate = declared_expression_columns(schema, predicate);
    let known_fields = schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();
    validate_index_expression(predicate, &known_fields)?;
    let types = super::super::coalesce_results::ResultTypes::for_source(
        &crate::sql::ast::QuerySource::Collection(
            IdentifierPath::parse(table).map_err(CassieError::Planner)?,
        ),
        &[],
        catalog,
        context,
    )?;
    super::super::boolean_contexts::validate_predicate(
        predicate,
        &types,
        "INDEX WHERE",
        catalog,
        context,
    )
}

pub(super) fn validate_index_expression(
    expr: &Expr,
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => {
            if known_fields.contains(name) {
                return Ok(());
            }
            return Err(CassieError::Planner(format!(
                "index expression references unknown field '{name}'"
            )));
        }
        Expr::Param(_) => {
            return Err(CassieError::Planner(
                "index expressions cannot reference query parameters".into(),
            ))
        }
        Expr::Exists(_) | Expr::Subquery(_) => {
            return Err(CassieError::Planner(
                "index expressions cannot contain subqueries".into(),
            ))
        }
        Expr::Function(function) => {
            return validate_index_expression_function(function, known_fields)
        }
        _ => {}
    }
    expr.try_visit_children(|child| validate_index_expression(child, known_fields))
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

/// Spells each index field the way the table declares it. Unquoted references
/// have already folded to lowercase; delimited references retain exact case.
/// An unknown name is kept so validation reports it.
fn declared_field_names(schema: &CollectionSchema, fields: Vec<String>) -> Vec<String> {
    fields
        .into_iter()
        .map(|field| declared_field_name(schema, field))
        .collect()
}

fn declared_field_name(schema: &CollectionSchema, field: String) -> String {
    let reference = crate::sql::ColumnIdentifierPath::reference_field_key(&field);
    schema
        .fields
        .iter()
        .find(|entry| crate::sql::ColumnIdentifierPath::stored_field_key(&entry.name) == reference)
        .map_or(field, |entry| entry.name.clone())
}

fn declared_expression_columns(schema: &CollectionSchema, expr: &Expr) -> Expr {
    match expr {
        Expr::Column(name) => Expr::Column(declared_field_name(schema, name.clone())),
        _ => expr.map_children(|child| declared_expression_columns(schema, child)),
    }
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
        .map(|field| crate::sql::ColumnIdentifierPath::stored_field_key(field))
        .collect::<std::collections::BTreeSet<_>>();
    for field in include_fields {
        let normalized = crate::sql::ColumnIdentifierPath::stored_field_key(field);
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

fn validate_scalar_index_key_options(
    statement: &CreateIndexStatement,
    schema: &CollectionSchema,
    fields: &[String],
    expressions: &[Expr],
) -> Result<(), CassieError> {
    let key_count = fields.len() + expressions.len();
    let key_options = (0..key_count)
        .map(|position| {
            (
                position,
                statement
                    .options
                    .get(&IndexMeta::key_option_name(position, "operator_class")),
                statement
                    .options
                    .get(&IndexMeta::key_option_name(position, "order")),
                statement
                    .options
                    .get(&IndexMeta::key_option_name(position, "nulls")),
            )
        })
        .filter(|(_, operator_class, order, nulls)| {
            operator_class.is_some() || order.is_some() || nulls.is_some()
        })
        .collect::<Vec<_>>();
    if key_options.is_empty() {
        return Ok(());
    }

    let access_method = statement
        .options
        .get(IndexMeta::ACCESS_METHOD_OPTION)
        .map_or("btree", String::as_str);
    if statement.kind != crate::catalog::IndexKind::Scalar
        || !access_method.eq_ignore_ascii_case("btree")
    {
        return Err(CassieError::Unsupported(
            "ordered keys and operator classes are supported only for scalar btree indexes".into(),
        ));
    }

    for (position, operator_class, order, nulls) in key_options {
        if order.is_some_and(|order| !matches!(order.as_str(), "asc" | "desc")) {
            return Err(CassieError::Unsupported(
                "unsupported btree key ordering option".into(),
            ));
        }
        if nulls.is_some_and(|nulls| !matches!(nulls.as_str(), "first" | "last")) {
            return Err(CassieError::Unsupported(
                "unsupported btree NULL ordering option".into(),
            ));
        }
        let Some(operator_class) = operator_class else {
            continue;
        };
        if !matches!(
            operator_class.as_str(),
            "text_pattern_ops" | "pg_catalog.text_pattern_ops"
        ) {
            return Err(CassieError::Unsupported(format!(
                "btree operator class '{operator_class}' is not supported"
            )));
        }
        let Some(field) = fields.get(position) else {
            return Err(CassieError::Unsupported(
                "text_pattern_ops requires a scalar text field key".into(),
            ));
        };
        let data_type = schema
            .fields
            .iter()
            .find(|candidate| candidate.name == *field)
            .map(|candidate| &candidate.data_type);
        if !data_type.is_some_and(|data_type| {
            matches!(
                data_type,
                DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
            )
        }) {
            return Err(CassieError::Unsupported(format!(
                "text_pattern_ops requires a text, char, or varchar key field, got '{field}'"
            )));
        }
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
            && crate::sql::ColumnIdentifierPath::stored_field_key(&metadata.field)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
    });
    if let Some(existing_fulltext_index) = existing_fulltext_index {
        let existing_index = catalog
            .get_index(table, name)
            .filter(|metadata| metadata.kind == crate::catalog::IndexKind::FullText)
            .filter(|metadata| metadata.field == existing_fulltext_index.field);

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
        if !seen_fields.insert(crate::sql::ColumnIdentifierPath::stored_field_key(field)) {
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
