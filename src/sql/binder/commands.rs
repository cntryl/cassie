use super::{
    bind_select, collect_item, normalize_relation_name, resolve_relation_name,
    resolve_relation_path, validate_expression, validate_function_calls, virtual_views,
    BindingContext, CassieError, Catalog, CatalogObjectKind, CollectionSchema, DataType, Expr,
    HashMap, HashSet, InsertSource, SelectItem,
};
use crate::sql::ast::IdentifierPath;

#[path = "commands/conflict.rs"]
mod conflict;
#[path = "commands/insert_boolean.rs"]
mod insert_boolean;
#[path = "commands/joined.rs"]
mod joined;
#[path = "commands/mutations.rs"]
mod mutations;
use mutations::infer_row_image_aliases;
pub(super) use mutations::{bind_merge, bind_truncate};

pub(super) fn bind_insert(
    mut statement: crate::sql::ast::InsertStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::InsertStatement, CassieError> {
    let table = resolve_relation_path(&statement.table, catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "INSERT requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if catalog.is_materialized_projection(&table) {
        statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
        return Ok(statement);
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;
    let qualifier = mutations::target_qualifier(&table, statement.alias.as_deref());
    infer_row_image_aliases(
        &statement.returning,
        &mut statement.returning_aliases,
        &qualifier,
    );
    if let Some(alias) = statement.alias.clone() {
        super::own_qualifier::strip_insert_qualifiers(&mut statement, &[alias]);
    }

    let mut seen_columns = HashSet::new();
    for column in &mut statement.columns {
        let column_name = column.trim().to_string();
        if column_name.is_empty() {
            return Err(CassieError::Planner(
                "INSERT column names cannot be empty".into(),
            ));
        }

        let Some(declared) = schema.fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::parse(&column_name)
                .is_ok_and(|reference| reference.matches_field_name(&field.name))
        }) else {
            return Err(CassieError::Planner(format!(
                "INSERT target column '{column_name}' does not exist in '{table}'"
            )));
        };
        let resolved_name = declared.name.clone();

        if !seen_columns.insert(resolved_name.clone()) {
            return Err(CassieError::Planner(format!(
                "INSERT column '{column_name}' is duplicated"
            )));
        }

        *column = resolved_name;
    }

    conflict::bind_on_conflict(
        statement.on_conflict.as_mut(),
        &schema,
        &table,
        catalog,
        context,
    )?;

    if let InsertSource::Select(select) = statement.source {
        let source = bind_select(*select, catalog, &super::CteScope::default(), context)?;
        statement.source = InsertSource::Select(Box::new(source));
    }

    insert_boolean::validate(&mut statement, &schema, catalog, context)?;

    validate_returning_items(
        &mut statement.returning,
        &schema,
        &table,
        "INSERT",
        catalog,
        context,
    )?;

    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    Ok(statement)
}

pub(super) fn bind_insert_boolean_parameters(
    statement: &mut crate::sql::ast::InsertStatement,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
) -> Result<(), CassieError> {
    // Preserve the existing special projection INSERT route.
    if catalog.is_materialized_projection(&statement.table) {
        return Ok(());
    }
    let schema = catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::CollectionNotFound(statement.table.to_string()))?;
    insert_boolean::validate_with_parameters(statement, &schema, catalog, context, parameter_types)
}

fn bind_boolean_filter(
    filter: Option<&mut Expr>,
    table: &str,
    label: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Some(filter) = filter else {
        return Ok(());
    };
    let types = super::coalesce_results::ResultTypes::for_source(
        &crate::sql::ast::QuerySource::Collection(
            IdentifierPath::parse(table).map_err(CassieError::Planner)?,
        ),
        &[],
        catalog,
        context,
    )?;
    super::boolean_contexts::validate_predicate(filter, &types, label, catalog, context)
}

pub(super) fn bind_update(
    mut statement: crate::sql::ast::UpdateStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::UpdateStatement, CassieError> {
    let qualifier = mutations::target_qualifier(&statement.table, statement.alias.as_deref());
    infer_row_image_aliases(
        &statement.returning,
        &mut statement.returning_aliases,
        &qualifier,
    );
    if joined::update_needs_joined_binding(&statement) {
        return joined::bind_joined_update(statement, catalog, context);
    }
    if let Some(alias) = statement.alias.take() {
        super::own_qualifier::strip_update_qualifiers(&mut statement, &[alias]);
    }
    let table = resolve_relation_path(&statement.table, catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "UPDATE requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if catalog.is_materialized_projection(&table) {
        statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
        return Ok(statement);
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let result_types = super::coalesce_results::ResultTypes::for_source(
        &crate::sql::ast::QuerySource::Collection(
            IdentifierPath::parse(&table).map_err(CassieError::Planner)?,
        ),
        &[],
        catalog,
        context,
    )?;
    let mut seen = HashSet::new();
    for (field, expression) in &mut statement.assignments {
        let normalized_field = field.trim().to_string();
        if normalized_field.is_empty() {
            return Err(CassieError::Planner(
                "UPDATE assignment names cannot be empty".into(),
            ));
        }
        let Some(declared) = schema.fields.iter().find(|entry| {
            crate::sql::ColumnIdentifierPath::parse(&normalized_field)
                .is_ok_and(|reference| reference.matches_field_name(&entry.name))
        }) else {
            return Err(CassieError::Planner(format!(
                "UPDATE assignment target '{normalized_field}' does not exist in '{table}'"
            )));
        };
        let resolved_name = declared.name.clone();

        if !seen.insert(resolved_name.clone()) {
            return Err(CassieError::Planner(format!(
                "UPDATE assignment target '{normalized_field}' is duplicated"
            )));
        }

        super::boolean_contexts::validate_value(
            expression,
            &result_types,
            Some(&declared.data_type),
            catalog,
            context,
        )?;
        *field = resolved_name;
    }

    validate_returning_items(
        &mut statement.returning,
        &schema,
        &table,
        "UPDATE",
        catalog,
        context,
    )?;

    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    super::own_qualifier::strip_update_own_qualifiers(&mut statement);
    if let Some(filter) = statement.filter.as_mut() {
        let field_types = crate::sql::source_field_type_map(
            &crate::sql::ast::QuerySource::Collection(statement.table.clone()),
            catalog,
        );
        super::select::canonicalize_typed_predicate_literals(filter, &field_types)?;
    }
    super::json_predicates::rewrite_filter(statement.filter.as_mut(), &schema);
    bind_boolean_filter(
        statement.filter.as_mut(),
        &statement.table,
        "UPDATE WHERE",
        catalog,
        context,
    )?;
    Ok(statement)
}

pub(super) fn bind_delete(
    mut statement: crate::sql::ast::DeleteStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::DeleteStatement, CassieError> {
    let qualifier = mutations::target_qualifier(&statement.table, statement.alias.as_deref());
    infer_row_image_aliases(
        &statement.returning,
        &mut statement.returning_aliases,
        &qualifier,
    );
    if joined::delete_needs_joined_binding(&statement) {
        return joined::bind_joined_delete(statement, catalog, context);
    }
    if let Some(alias) = statement.alias.take() {
        super::own_qualifier::strip_delete_qualifiers(&mut statement, &[alias]);
    }
    let table = resolve_relation_path(&statement.table, catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DELETE requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if catalog.is_materialized_projection(&table) {
        statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
        return Ok(statement);
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }
    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    validate_returning_items(
        &mut statement.returning,
        &schema,
        &table,
        "DELETE",
        catalog,
        context,
    )?;

    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    super::own_qualifier::strip_delete_own_qualifiers(&mut statement);
    if let Some(filter) = statement.filter.as_mut() {
        let field_types = crate::sql::source_field_type_map(
            &crate::sql::ast::QuerySource::Collection(statement.table.clone()),
            catalog,
        );
        super::select::canonicalize_typed_predicate_literals(filter, &field_types)?;
    }
    super::json_predicates::rewrite_filter(statement.filter.as_mut(), &schema);
    bind_boolean_filter(
        statement.filter.as_mut(),
        &statement.table,
        "DELETE WHERE",
        catalog,
        context,
    )?;
    Ok(statement)
}

pub(super) fn bind_create_rollup(
    mut statement: crate::sql::ast::CreateRollupStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateRollupStatement, CassieError> {
    let name = super::normalize_new_relation_name(statement.name.trim(), context, catalog)?;
    if name.is_empty() {
        return Err(CassieError::Planner("CREATE ROLLUP requires a name".into()));
    }
    if catalog.get_rollup(&name).is_some() {
        if statement.if_not_exists {
            statement.name = name;
            return Ok(statement);
        }
        return Err(CassieError::Planner(format!(
            "rollup '{name}' already exists"
        )));
    }

    crate::sql::definition_guard::create_rollup(&statement)?;

    let source = resolve_relation_name(statement.source.trim(), catalog, context)?;
    if virtual_views::schema(&source).is_some() || catalog.get_view(&source).is_some() {
        return Err(CassieError::Unsupported(format!(
            "rollup source '{source}' must be a base collection"
        )));
    }

    if !statement.bucket.name.eq_ignore_ascii_case("time_bucket") {
        return Err(CassieError::Planner(
            "CREATE ROLLUP USING requires time_bucket".into(),
        ));
    }
    if !(2..=3).contains(&statement.bucket.args.len()) {
        return Err(CassieError::Planner(
            "time_bucket rollups require width, timestamp[, origin]".into(),
        ));
    }
    let Expr::Column(timestamp_field) = &statement.bucket.args[1] else {
        return Err(CassieError::Planner(
            "time_bucket rollup timestamp argument must be a column".into(),
        ));
    };

    let schema = catalog
        .get_schema(&source)
        .ok_or_else(|| CassieError::CollectionNotFound(source.clone()))?;
    let known_fields = schema
        .fields
        .iter()
        .map(|field| crate::sql::ColumnIdentifierPath::stored_field_key(&field.name))
        .collect::<HashSet<_>>();
    let mut expression_fields = known_fields.clone();
    expression_fields.extend(super::select::base_table_fields(&source, &schema));
    if !known_fields.contains(&crate::sql::ColumnIdentifierPath::reference_field_key(
        timestamp_field,
    )) {
        return Err(CassieError::Planner(format!(
            "rollup timestamp column '{timestamp_field}' does not exist in '{source}'"
        )));
    }

    for expr in &statement.group_by {
        let Expr::Column(name) = expr else {
            return Err(CassieError::Planner(
                "rollup GROUP BY supports source columns only".into(),
            ));
        };
        if !known_fields.contains(&crate::sql::ColumnIdentifierPath::reference_field_key(name)) {
            return Err(CassieError::Planner(format!(
                "rollup group column '{name}' does not exist in '{source}'"
            )));
        }
    }

    for item in &statement.aggregates {
        let SelectItem::Function { function, .. } = item else {
            return Err(CassieError::Planner(
                "rollup AGGREGATES supports aggregate functions only".into(),
            ));
        };
        if !crate::sql::functions::is_aggregate_function(&function.name) {
            return Err(CassieError::Unsupported(format!(
                "rollup aggregate '{}' is not supported",
                function.name
            )));
        }
        if !(function.name.eq_ignore_ascii_case("count")
            && matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*"))
        {
            for arg in &function.args {
                validate_expression(arg, &expression_fields, &HashSet::new(), false)?;
            }
        }
    }

    if let Some(filter) = &statement.filter {
        validate_expression(filter, &expression_fields, &HashSet::new(), false)?;
    }
    bind_boolean_filter(
        statement.filter.as_mut(),
        &source,
        "ROLLUP WHERE",
        catalog,
        context,
    )?;

    statement.name = name;
    statement.source = source;
    Ok(statement)
}

pub(super) fn bind_create_retention_policy(
    mut statement: crate::sql::ast::CreateRetentionPolicyStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateRetentionPolicyStatement, CassieError> {
    let name = normalize_relation_name(statement.name.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE RETENTION POLICY requires a name".into(),
        ));
    }
    if catalog.get_retention_policy(&name).is_some() {
        if statement.if_not_exists {
            statement.name = name;
            return Ok(statement);
        }
        return Err(CassieError::Planner(format!(
            "retention policy '{name}' already exists"
        )));
    }

    let collection = resolve_relation_name(statement.collection.trim(), catalog, context)?;
    let field = statement.timestamp_field.trim().to_string();
    validate_retention_target(catalog, &collection, &field)?;
    validate_retention_duration(&statement.retention_duration)?;

    statement.name = name;
    statement.collection = collection;
    statement.timestamp_field = field;
    Ok(statement)
}

pub(super) fn bind_alter_retention_policy(
    mut statement: crate::sql::ast::AlterRetentionPolicyStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::AlterRetentionPolicyStatement, CassieError> {
    let name = super::resolve_existing_name(statement.name.trim(), context, |name| {
        catalog.get_retention_policy(name).is_some()
    })?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "ALTER RETENTION POLICY requires a name".into(),
        ));
    }
    if catalog.get_retention_policy(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::RetentionPolicy,
            name,
        });
    }
    validate_retention_duration(&statement.retention_duration)?;
    statement.name = name;
    Ok(statement)
}

pub(super) fn bind_enforce_retention_policy(
    mut statement: crate::sql::ast::EnforceRetentionPolicyStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::EnforceRetentionPolicyStatement, CassieError> {
    let name = normalize_relation_name(statement.name.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "ENFORCE RETENTION POLICY requires a name".into(),
        ));
    }
    if catalog.get_retention_policy(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::RetentionPolicy,
            name,
        });
    }
    validate_retention_timestamp(&statement.at)?;
    statement.name = name;
    Ok(statement)
}

fn validate_retention_target(
    catalog: &Catalog,
    collection: &str,
    field: &str,
) -> Result<(), CassieError> {
    if collection.is_empty() || !catalog.exists(collection) {
        return Err(CassieError::CollectionNotFound(collection.to_string()));
    }
    if virtual_views::schema(collection).is_some() || catalog.get_view(collection).is_some() {
        return Err(CassieError::Unsupported(format!(
            "retention policy target '{collection}' must be a base collection"
        )));
    }
    match catalog.field_type(collection, field) {
        Some(DataType::Text | DataType::Timestamp) => Ok(()),
        Some(_) => Err(CassieError::Planner(format!(
            "retention timestamp field '{field}' must be text or timestamp"
        ))),
        None => Err(CassieError::Planner(format!(
            "retention timestamp field '{field}' does not exist in '{collection}'"
        ))),
    }
}

fn validate_retention_duration(raw: &str) -> Result<(), CassieError> {
    let mut parts = raw.split_whitespace();
    let amount = parts
        .next()
        .ok_or_else(|| CassieError::Planner("retention duration cannot be empty".into()))?
        .parse::<u64>()
        .map_err(|_| CassieError::Planner("retention duration requires a number".into()))?;
    let unit = parts
        .next()
        .ok_or_else(|| CassieError::Planner("retention duration requires a unit".into()))?;
    if amount == 0 || parts.next().is_some() {
        return Err(CassieError::Planner(
            "retention duration must be '<positive number> <unit>'".into(),
        ));
    }
    match unit.to_ascii_lowercase().as_str() {
        "minute" | "minutes" | "hour" | "hours" | "day" | "days" => Ok(()),
        _ => Err(CassieError::Planner(
            "retention duration supports minutes, hours, or days".into(),
        )),
    }
}

fn validate_retention_timestamp(raw: &str) -> Result<(), CassieError> {
    crate::types::temporal::parse_timestamp_utc(raw)
        .map(|_| ())
        .map_err(|_| CassieError::Planner("retention enforcement AT must be RFC3339".into()))
}

pub(super) fn validate_returning_items(
    returning: &mut [SelectItem],
    schema: &CollectionSchema,
    table: &str,
    operation: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let mut known_fields = schema
        .fields
        .iter()
        .map(|field| crate::sql::ColumnIdentifierPath::stored_field_key(&field.name))
        .collect::<HashSet<_>>();
    known_fields.extend(super::select::base_table_fields(table, schema));

    let result_types = super::coalesce_results::ResultTypes::for_source(
        &crate::sql::ast::QuerySource::Collection(
            IdentifierPath::parse(table).map_err(CassieError::Planner)?,
        ),
        &[],
        catalog,
        context,
    )?;
    let mut functions = Vec::new();
    for item in returning {
        result_types.item(item)?;
        match item {
            SelectItem::Wildcard => {}
            SelectItem::Column { name, .. } => {
                if crate::types::row_identity::is_row_identity_column(name) {
                    continue;
                }

                if !schema.fields.iter().any(|field| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
                        == crate::sql::ColumnIdentifierPath::reference_field_key(name)
                }) {
                    return Err(CassieError::Planner(format!(
                        "{operation} RETURNING column '{name}' does not exist in '{table}'"
                    )));
                }
            }
            SelectItem::Function { function, .. } => {
                validate_expression(
                    &Expr::Function(Box::new(function.clone())),
                    &known_fields,
                    &HashSet::new(),
                    false,
                )?;
                for argument in function.expressions_mut() {
                    super::boolean_contexts::validate_value(
                        argument,
                        &result_types,
                        None,
                        catalog,
                        context,
                    )?;
                }
                collect_item(item, &mut functions);
            }
            SelectItem::Expr { expr, .. } => {
                validate_expression(expr, &known_fields, &HashSet::new(), false)?;
                super::boolean_contexts::validate_value(
                    expr,
                    &result_types,
                    None,
                    catalog,
                    context,
                )?;
            }
            SelectItem::WindowFunction { .. } => {
                return Err(CassieError::Planner(format!(
                    "{operation} RETURNING does not support window functions"
                )));
            }
        }
    }

    validate_function_calls(functions, catalog, context)
}
