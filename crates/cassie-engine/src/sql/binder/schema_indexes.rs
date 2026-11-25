use super::{
    bm25, normalize_relation_name, resolve_relation_path, schema_index_options, BindingContext,
    CassieError, Catalog, CollectionSchema, Expr, HashSet,
};
use crate::catalog::{derive_scoped_name, local_name, parse_name, IndexMeta, ParsedName};
use crate::sql::ast::{CreateIndexStatement, IdentifierPath};
use crate::types::DataType;

#[path = "index_generated_name.rs"]
mod index_generated_name;

#[path = "schema_index_duplicate.rs"]
mod duplicate;

#[path = "schema_index_fulltext.rs"]
mod fulltext;

pub(super) fn bind_create_index(
    mut statement: CreateIndexStatement,
    catalog: &Catalog,
    context: &BindingContext,
    controls: Option<&crate::runtime::QueryExecutionControls>,
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
            ParsedName::Unqualified(name) => scoped_index_name(&table, name),
            _ => normalize_relation_name(statement.name.trim(), context)?,
        }
    };
    if !name.is_empty() {
        catalog.reject_composite_relation_name(&name)?;
    }
    if !name.is_empty() && statement.if_not_exists && catalog.get_index(&table, &name).is_some() {
        duplicate::validate(&statement, catalog, &table, controls)?;
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
    reject_composite_index_columns(&schema, &fields)?;
    let expressions = declared_index_expressions(&schema, &expressions);
    if name.is_empty() {
        name =
            index_generated_name::generate(&table, &fields, catalog, &context.database, controls)?;
    }
    validate_index_shape(&statement, &fields, &expressions)?;

    let include_fields = declared_field_names(&schema, normalize_fields(&statement.include_fields));
    validate_include_fields(&statement, &schema, &fields, &include_fields)?;
    validate_index_fields_and_expressions(&schema, &fields, &expressions)?;
    let search_path = context
        .relation_search_order()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    bind_index_method_options(
        &mut statement,
        IndexMethodBinding {
            catalog,
            schema: &schema,
            database: &context.database,
            search_path: &search_path,
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

    if !statement.if_not_exists
        && catalog.index_relation_name_occupied(&name, &context.database, false, controls)?
    {
        return Err(crate::app::DiagnosticError::new(
            "42P07",
            format!("relation \"{}\" already exists", local_name(&name)),
        )
        .into());
    }

    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    statement.name = name;
    statement.fields = fields;
    statement.expressions = expressions;
    statement.include_fields = include_fields;
    Ok(statement)
}

fn reject_composite_index_columns(
    schema: &CollectionSchema,
    fields: &[String],
) -> Result<(), CassieError> {
    for field in fields {
        if schema.fields.iter().any(|entry| {
            entry.name.eq_ignore_ascii_case(field)
                && matches!(entry.data_type, crate::types::DataType::Composite(_))
        }) {
            return Err(CassieError::Unsupported(format!(
                "indexes on composite column \"{field}\" are not supported in this version"
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct IndexMethodBinding<'a> {
    catalog: &'a Catalog,
    schema: &'a CollectionSchema,
    database: &'a str,
    search_path: &'a [String],
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
        database,
        search_path,
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
            statement,
            &schema_index_options::IndexBindingScope {
                catalog,
                database,
                search_path,
            },
            schema,
            table,
            name,
            fields,
        )?,
        crate::catalog::IndexKind::FullText => {
            fulltext::bind_fulltext_index_options(statement, catalog, schema, table, name, fields)?;
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

    let (database, schema, _) = match reference {
        ParsedName::Unqualified(local) => (None, None, local),
        ParsedName::SchemaQualified { schema, name } => (None, Some(schema), name),
        ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        } => (Some(database), Some(schema), name),
    };
    let identifier = IdentifierPath::parse(raw).map_err(CassieError::Planner)?;
    let last = identifier
        .components()
        .last()
        .ok_or_else(|| CassieError::Planner("empty index name".to_owned()))?;
    let local = if last.is_delimited() {
        last.value().to_owned()
    } else {
        last.value().to_ascii_lowercase()
    };
    let indexes = catalog
        .all_indexes_snapshot()
        .into_iter()
        .filter(|index| {
            catalog
                .materialized_projection_for_output(&index.collection)
                .is_none_or(|projection| {
                    !catalog.is_sql_materialized_view(&projection.collection)
                        || projection.active_output_collection() == Some(index.collection.as_str())
                })
        })
        .collect::<Vec<_>>();
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
                index_name_matches_scope(
                    index,
                    context,
                    database.as_deref(),
                    &schema,
                    &local,
                    last.is_delimited(),
                )
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
    delimited: bool,
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
        && if delimited {
            index_local == local
        } else {
            index_local.eq_ignore_ascii_case(local)
        }
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
            return Err(CassieError::UndefinedColumn(name.clone()));
        }
        Expr::Param(_) => {
            return Err(CassieError::Planner(
                "index expressions cannot reference query parameters".into(),
            ));
        }
        Expr::Exists(_) | Expr::Subquery(_) => {
            return Err(CassieError::Planner(
                "index expressions cannot contain subqueries".into(),
            ));
        }
        Expr::Function(function) => {
            return validate_index_expression_function(function, known_fields);
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
    if matches!(
        statement.kind,
        crate::catalog::IndexKind::FullText | crate::catalog::IndexKind::Vector
    ) {
        if statement.unique {
            return Err(CassieError::Unsupported(
                "unique indexes are supported only for scalar btree indexes".into(),
            ));
        }
        if statement.predicate.is_some() {
            return Err(CassieError::Unsupported(
                "partial predicates are supported only for scalar btree indexes".into(),
            ));
        }
    }
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
            return Err(CassieError::UndefinedColumn(field.clone()));
        }
    }
    Ok(())
}

fn validate_index_fields_and_expressions(
    schema: &CollectionSchema,
    fields: &[String],
    expressions: &[Expr],
) -> Result<(), CassieError> {
    for field in fields {
        if !schema.fields.iter().any(|entry| entry.name == *field) {
            return Err(CassieError::UndefinedColumn(field.clone()));
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
    if statement.kind == crate::catalog::IndexKind::Vector
        && statement
            .options
            .get(IndexMeta::ACCESS_METHOD_OPTION)
            .is_some_and(|method| matches!(method.as_str(), "hnsw" | "ivfflat"))
    {
        return Ok(());
    }
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
    if normalized == crate::sql::ast::FIELD_ACCESS_FUNCTION {
        return Err(CassieError::Unsupported(
            "an index on a field of a composite column is not supported".into(),
        ));
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

pub(super) fn bind_index_rename_target(
    raw: &str,
    index: &IndexMeta,
    catalog: &Catalog,
) -> Result<String, CassieError> {
    let ParsedName::Unqualified(local) = parse_name(raw).map_err(CassieError::Planner)? else {
        return Err(CassieError::Planner(
            "RENAME TO requires an unqualified new name".into(),
        ));
    };
    let owner = catalog
        .materialized_projection_for_output(&index.collection)
        .map_or_else(
            || index.collection.clone(),
            |projection| projection.collection,
        );
    let mut parts = IdentifierPath::parse(&owner)
        .map_err(CassieError::Planner)?
        .values()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    *parts.last_mut().expect("owner has a local component") = local;
    Ok(IdentifierPath::from_values(parts).into_string())
}

/// Places an unqualified index name beside its table. A name that needs quoting
/// (dots, quotes, whitespace) keeps its quotes where no database qualifier
/// re-escapes it, so `"Case.Index"` is not read back as `Case`.`Index`.
fn scoped_index_name(table: &str, name: String) -> String {
    let quoted = crate::catalog::canonical_identifier_component(&name);
    match parse_name(table) {
        Ok(ParsedName::DatabaseQualified { .. }) => derive_scoped_name(table, |_| name),
        _ => derive_scoped_name(table, |_| quoted),
    }
}
