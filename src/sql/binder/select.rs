use super::{
    bind_recursive_cte_query, bind_statement, collect_projection_aliases, mem, qualified_fields,
    resolve_relation_name, validate_distinct_on_order_prefix, validate_expression,
    validate_expression_operand_families, validate_expression_references, validate_functions,
    validate_order_by_references, validate_projection_references, validate_select_operand_families,
    virtual_views, BindingContext, CassieError, Catalog, CteQuery, CteScope, DataType, Expr,
    FieldSchema, FunctionCall, HashMap, HashSet, QuerySource, QueryStatement, Schema, SelectItem,
    SelectSet, SelectStatement,
};
use crate::types::row_identity::{
    is_legacy_id_column, is_row_identity_column, LEGACY_ID_COLUMN, ROW_IDENTITY_COLUMN,
};

pub(super) fn bind_select(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    context: &BindingContext,
) -> Result<SelectStatement, CassieError> {
    bind_select_with_lateral_fields(select, catalog, outer_scope, &HashSet::new(), context)
}

pub(super) fn bind_select_with_lateral_fields(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    lateral_fields: &HashSet<String>,
    context: &BindingContext,
) -> Result<SelectStatement, CassieError> {
    let mut scope = outer_scope.clone();
    let mut local_names = HashSet::new();
    let mut select = select;
    let ctes = mem::take(&mut select.ctes);
    let set = mem::take(&mut select.set);

    let mut bound_ctes = Vec::with_capacity(ctes.len());
    for cte in ctes {
        let cte_name = cte.name.trim();
        if cte_name.is_empty() {
            return Err(CassieError::Planner("CTE name cannot be empty".into()));
        }
        let cte_name_lc = cte_name.to_ascii_lowercase();
        if !local_names.insert(cte_name_lc.clone()) {
            return Err(CassieError::Planner(format!(
                "duplicate CTE name '{cte_name}'"
            )));
        }

        let declared_aliases = cte.aliases.clone();
        let query = match cte.query {
            CteQuery::Simple(next) => {
                CteQuery::Simple(Box::new(bind_statement(*next, catalog, &scope, context)?))
            }
            CteQuery::Recursive {
                operator,
                base,
                recursive,
            } => bind_recursive_cte_query(
                CteQuery::Recursive {
                    operator,
                    base,
                    recursive,
                },
                &declared_aliases,
                catalog,
                &scope,
                cte_name,
                context,
            )?,
        };

        let visible_fields = cte_output_fields(&query)?;
        let aliases = if cte.aliases.is_empty() {
            visible_fields
        } else {
            if visible_fields.len() != cte.aliases.len() {
                return Err(CassieError::Planner(format!(
                    "CTE '{cte_name}' alias count does not match output columns"
                )));
            }

            cte.aliases
                .iter()
                .map(|alias| alias.to_ascii_lowercase())
                .collect()
        };
        scope.insert(cte_name_lc, aliases.clone());

        bound_ctes.push(crate::sql::ast::CommonTableExpression {
            name: cte.name,
            aliases: if declared_aliases.is_empty() {
                aliases
            } else {
                declared_aliases
            },
            query,
        });
    }

    let source = bind_query_source_with_lateral_fields(
        select.source.clone(),
        catalog,
        &scope,
        lateral_fields,
        context,
    )?;
    let mut known_fields = source_fields(catalog, &source, &scope)?;
    known_fields.extend(lateral_fields.iter().cloned());
    select.source = source;
    select.ctes = bound_ctes;

    let field_types = crate::sql::source_field_type_map(&select.source, catalog);
    if let Some(filter) = select.filter.as_mut() {
        canonicalize_typed_predicate_literals(filter, &field_types)?;
    }
    super::case_unify::unify_select_case_results(&mut select, &field_types);

    let projection_aliases = collect_projection_aliases(&select);
    validate_bound_select_references(&select, &known_fields, &projection_aliases)?;
    validate_select_operand_families(&select, catalog)?;

    if let Some(set) = set {
        let right = bind_select(*set.right, catalog, &scope, context)?;
        select.set = Some(Box::new(SelectSet {
            operator: set.operator,
            right: Box::new(right),
        }));
    }

    validate_functions(&select, catalog)?;

    Ok(select)
}

fn canonicalize_typed_predicate_literals(
    expr: &mut Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    match expr {
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            if let Some(operand) = operand {
                canonicalize_typed_predicate_literals(operand, field_types)?;
            }
            for (when, then) in branches {
                canonicalize_typed_predicate_literals(when, field_types)?;
                canonicalize_typed_predicate_literals(then, field_types)?;
            }
            if let Some(else_expr) = else_expr {
                canonicalize_typed_predicate_literals(else_expr, field_types)?;
            }
            Ok(())
        }
        Expr::Binary { left, right, .. } => {
            canonicalize_column_literal_pair(left, right, field_types)?;
            canonicalize_typed_predicate_literals(left, field_types)?;
            canonicalize_typed_predicate_literals(right, field_types)
        }
        Expr::InList { expr, values, .. } => {
            for value in values.iter_mut() {
                canonicalize_column_literal_pair(expr, value, field_types)?;
                canonicalize_typed_predicate_literals(value, field_types)?;
            }
            canonicalize_typed_predicate_literals(expr, field_types)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            canonicalize_column_literal_pair(expr, low, field_types)?;
            canonicalize_column_literal_pair(expr, high, field_types)?;
            canonicalize_typed_predicate_literals(expr, field_types)?;
            canonicalize_typed_predicate_literals(low, field_types)?;
            canonicalize_typed_predicate_literals(high, field_types)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            canonicalize_typed_predicate_literals(expr, field_types)
        }
        Expr::Function(function) => {
            for argument in &mut function.args {
                canonicalize_typed_predicate_literals(argument, field_types)?;
            }
            Ok(())
        }
        Expr::Column(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_)
        | Expr::Exists(_) => Ok(()),
    }
}

fn canonicalize_column_literal_pair(
    left: &mut Expr,
    right: &mut Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    let (
        (Expr::Column(column), Expr::StringLiteral(literal))
        | (Expr::StringLiteral(literal), Expr::Column(column)),
    ) = ((left, right),)
    else {
        return Ok(());
    };
    match crate::sql::field_type_for_column(field_types, column) {
        Some(DataType::Uuid) => {
            *literal = uuid::Uuid::parse_str(literal)
                .map_err(|_| CassieError::Planner(format!("invalid UUID literal '{literal}'")))?
                .to_string();
        }
        Some(DataType::Bytea) => {
            let digits = literal.strip_prefix("\\x").ok_or_else(|| {
                CassieError::Planner(format!("invalid BYTEA literal '{literal}'"))
            })?;
            if digits.len() % 2 != 0 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(CassieError::Planner(format!(
                    "invalid BYTEA literal '{literal}'"
                )));
            }
            *literal = format!("\\x{}", digits.to_ascii_lowercase());
        }
        _ => {}
    }
    Ok(())
}

fn validate_bound_select_references(
    select: &SelectStatement,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
) -> Result<(), CassieError> {
    validate_projection_references(&select.projection, known_fields)?;
    validate_grouped_projection(&select.projection, &select.group_by)?;
    validate_expression_references(
        select.filter.as_ref(),
        known_fields,
        projection_aliases,
        false,
    )?;
    for group_expr in &select.group_by {
        validate_expression(group_expr, known_fields, projection_aliases, false)?;
    }
    for distinct_expr in &select.distinct_on {
        validate_expression(distinct_expr, known_fields, projection_aliases, false)?;
    }
    validate_expression_references(
        select.having.as_ref(),
        known_fields,
        projection_aliases,
        false,
    )?;
    validate_order_by_references(&select.order, known_fields, projection_aliases)?;
    validate_distinct_on_order_prefix(&select.distinct_on, &select.order)?;
    Ok(())
}

fn validate_grouped_projection(
    projection: &[SelectItem],
    group_by: &[Expr],
) -> Result<(), CassieError> {
    if group_by.is_empty() {
        return Ok(());
    }
    for item in projection {
        let expression = match item {
            SelectItem::Wildcard => {
                return Err(CassieError::Planner(
                    "wildcard projection must appear in the GROUP BY clause".to_string(),
                ));
            }
            SelectItem::Column { name, .. } => Expr::Column(name.clone()),
            SelectItem::Function { function, .. }
                if crate::sql::functions::is_aggregate_function(&function.name) =>
            {
                continue;
            }
            SelectItem::Function { function, .. } => Expr::Function(function.clone()),
            SelectItem::Expr { expr, .. } => expr.clone(),
            SelectItem::WindowFunction { .. } => continue,
        };
        if let Some(column) = first_ungrouped_column(&expression, group_by) {
            return Err(CassieError::Planner(format!(
                "column '{column}' must appear in the GROUP BY clause or be used in an aggregate function"
            )));
        }
    }
    Ok(())
}

/// Returns the first column that is neither grouped nor inside an aggregate.
/// Sub-expressions that structurally match a `GROUP BY` expression (such as
/// a grouped CASE) count as grouped.
fn first_ungrouped_column<'a>(expr: &'a Expr, group_by: &[Expr]) -> Option<&'a str> {
    if group_by.iter().any(|grouped| grouped.structurally_eq(expr)) {
        return None;
    }
    match expr {
        Expr::Column(column) => Some(column),
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) =>
        {
            None
        }
        _ => expr.find_map_child(|child| first_ungrouped_column(child, group_by)),
    }
}

pub(super) fn validate_recursive_cte_shape(
    base: &crate::sql::ast::ParsedStatement,
    recursive: &crate::sql::ast::ParsedStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    cte_name: &str,
    aliases: &[String],
) -> Result<(), CassieError> {
    let QueryStatement::Select(base_select) = &base.statement else {
        return Err(CassieError::Planner(
            "recursive CTE anchor must be a SELECT statement".into(),
        ));
    };
    let QueryStatement::Select(recursive_select) = &recursive.statement else {
        return Err(CassieError::Planner(
            "recursive CTE term must be a SELECT statement".into(),
        ));
    };
    if base_select.set.is_some() || recursive_select.set.is_some() {
        return Err(CassieError::Planner(format!(
            "recursive CTE '{cte_name}' has unsupported nested set operation"
        )));
    }

    let user_functions = catalog
        .list_functions()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function))
        .collect::<HashMap<_, _>>();
    let outer_schemas = outer_scope
        .iter()
        .map(|(name, aliases)| {
            (
                name.clone(),
                Schema {
                    fields: aliases
                        .iter()
                        .map(|alias| FieldSchema {
                            name: alias.clone(),
                            data_type: DataType::Null,
                            nullable: true,
                        })
                        .collect(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let mut base_schema = super::inference::infer_select_schema_with_scope(
        base_select,
        catalog,
        &outer_schemas,
        &user_functions,
    )?;
    if base_schema.fields.len() != aliases.len() {
        return Err(CassieError::Planner(format!(
            "recursive CTE '{cte_name}' column count mismatch: {} != {}",
            base_schema.fields.len(),
            aliases.len()
        )));
    }
    for (field, alias) in base_schema.fields.iter_mut().zip(aliases) {
        field.name.clone_from(alias);
    }
    let mut recursive_schemas = outer_schemas;
    recursive_schemas.insert(cte_name.to_ascii_lowercase(), base_schema.clone());
    let recursive_schema = super::inference::infer_select_schema_with_scope(
        recursive_select,
        catalog,
        &recursive_schemas,
        &user_functions,
    )?;

    if base_schema.fields.len() != recursive_schema.fields.len() {
        return Err(CassieError::Planner(format!(
            "recursive CTE '{cte_name}' column count mismatch: {} != {}",
            base_schema.fields.len(),
            recursive_schema.fields.len()
        )));
    }

    for (index, (base_field, recursive_field)) in base_schema
        .fields
        .iter()
        .zip(recursive_schema.fields.iter())
        .enumerate()
    {
        if !recursive_types_compatible(&base_field.data_type, &recursive_field.data_type) {
            return Err(CassieError::Planner(format!(
                "recursive CTE '{cte_name}' column {} has incompatible types: {} != {}",
                index + 1,
                base_field.data_type.type_name(),
                recursive_field.data_type.type_name()
            )));
        }
    }
    Ok(())
}

fn recursive_types_compatible(base: &DataType, recursive: &DataType) -> bool {
    if matches!(base, DataType::Null) || matches!(recursive, DataType::Null) {
        return true;
    }
    match (base, recursive) {
        (
            DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float,
            DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float,
        )
        | (
            DataType::Text | DataType::Char { .. } | DataType::Varchar { .. },
            DataType::Text | DataType::Char { .. } | DataType::Varchar { .. },
        ) => true,
        (DataType::Array(base), DataType::Array(recursive)) => {
            recursive_types_compatible(base, recursive)
        }
        _ => base == recursive,
    }
}

pub(super) fn cte_output_fields(cte_query: &CteQuery) -> Result<Vec<String>, CassieError> {
    let query = match cte_query {
        CteQuery::Simple(statement) => statement,
        CteQuery::Recursive { base, .. } => base,
    };

    let QueryStatement::Select(select) = &query.statement else {
        return Err(CassieError::Planner(
            "CTE body must be a SELECT statement".into(),
        ));
    };
    if select.projection.iter().any(matches_wildcard) {
        return Ok(vec!["*".into()]);
    }

    Ok(projected_column_names(&select.projection))
}

pub(super) fn projected_column_names(projection: &[SelectItem]) -> Vec<String> {
    projection
        .iter()
        .map(|item| match item {
            SelectItem::Wildcard => "*".to_string(),
            SelectItem::Column {
                name: _,
                alias: Some(alias),
                ..
            } => alias.to_ascii_lowercase(),
            SelectItem::Column { name, alias: None } => name.to_ascii_lowercase(),
            SelectItem::Function { function, alias } => alias
                .as_deref()
                .unwrap_or(&function.name)
                .to_ascii_lowercase(),
            SelectItem::Expr { alias, .. } => {
                alias.as_deref().unwrap_or("expr").to_ascii_lowercase()
            }
            SelectItem::WindowFunction { function, alias } => alias
                .as_deref()
                .unwrap_or(&function.name)
                .to_ascii_lowercase(),
        })
        .collect()
}

pub(super) fn matches_wildcard(item: &SelectItem) -> bool {
    matches!(item, SelectItem::Wildcard)
}

pub(super) fn bind_query_source_with_lateral_fields(
    source: QuerySource,
    catalog: &Catalog,
    scope: &CteScope,
    lateral_fields: &HashSet<String>,
    context: &BindingContext,
) -> Result<QuerySource, CassieError> {
    match source {
        QuerySource::Collection(name) => {
            let source_name_lc = name.to_ascii_lowercase();
            if scope.contains_key(&source_name_lc) {
                Ok(QuerySource::Cte(name))
            } else {
                Ok(QuerySource::Collection(resolve_relation_name(
                    &name, catalog, context,
                )?))
            }
        }
        QuerySource::Cte(name) => Ok(QuerySource::Cte(name)),
        QuerySource::SingleRow => Ok(QuerySource::SingleRow),
        QuerySource::TableFunction {
            name,
            function,
            lateral,
        } => {
            validate_graph_table_function(&function, lateral_fields)?;
            if let Some(graph_name) = literal_string_arg(&function, 0) {
                if !catalog.graph_exists(&graph_name) {
                    return Err(CassieError::Planner(format!(
                        "graph '{graph_name}' does not exist"
                    )));
                }
            }
            Ok(QuerySource::TableFunction {
                name,
                function,
                lateral,
            })
        }
        QuerySource::Subquery {
            alias,
            select,
            lateral,
        } => {
            let empty = HashSet::new();
            let visible_lateral_fields = if lateral { lateral_fields } else { &empty };
            let select = bind_select_with_lateral_fields(
                *select,
                catalog,
                scope,
                visible_lateral_fields,
                context,
            )?;
            Ok(QuerySource::Subquery {
                alias,
                select: Box::new(select),
                lateral,
            })
        }
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let left = bind_query_source_with_lateral_fields(
                *left,
                catalog,
                scope,
                lateral_fields,
                context,
            )?;
            let mut right_lateral_fields = lateral_fields.clone();
            right_lateral_fields.extend(source_fields(catalog, &left, scope)?);
            let right = bind_query_source_with_lateral_fields(
                *right,
                catalog,
                scope,
                &right_lateral_fields,
                context,
            )?;
            let joined = QuerySource::Join {
                left: Box::new(left),
                right: Box::new(right),
                kind,
                on: on.clone(),
            };
            let known_fields = source_fields(catalog, &joined, scope)?;
            validate_expression(&on, &known_fields, &HashSet::new(), false)?;
            let field_types = crate::sql::source_field_type_map(&joined, catalog);
            validate_expression_operand_families(&on, &field_types)?;
            Ok(joined)
        }
    }
}

pub(super) fn source_fields(
    catalog: &Catalog,
    source: &QuerySource,
    scope: &CteScope,
) -> Result<HashSet<String>, CassieError> {
    match source {
        QuerySource::Collection(name) => {
            if let Some(fields) = virtual_views::schema(name) {
                Ok(qualified_fields(
                    name,
                    fields.into_iter().map(|(field, _)| field),
                ))
            } else if let Some(projection) = catalog.get_materialized_projection(name) {
                let materialized = projection.materialized.ok_or_else(|| {
                    CassieError::Planner(format!(
                        "materialized projection '{name}' is missing output schema"
                    ))
                })?;
                Ok(qualified_fields(
                    name,
                    materialized
                        .output_schema
                        .fields
                        .into_iter()
                        .map(|field| field.name.to_ascii_lowercase()),
                ))
            } else {
                let schema = catalog
                    .get_schema(name)
                    .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;
                Ok(base_table_fields(name, &schema))
            }
        }
        QuerySource::Cte(name) => scope
            .get(&name.to_ascii_lowercase())
            .cloned()
            .map(|fields| qualified_fields(name, fields))
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone())),
        QuerySource::SingleRow => Ok(HashSet::new()),
        QuerySource::TableFunction { name, .. } => Ok(qualified_fields(
            name,
            table_function_columns(name)
                .into_iter()
                .map(|(name, _)| name),
        )),
        QuerySource::Subquery { alias, select, .. } => Ok(qualified_fields(
            alias,
            projected_column_names(&select.projection),
        )),
        QuerySource::Join { left, right, .. } => {
            let mut fields = source_fields(catalog, left, scope)?;
            let right_fields = source_fields(catalog, right, scope)?;
            // Both sides of a join resolve the internal identity the same way
            // (see `planner::logical::reserved_id`), so `_id`, and a bare `id`
            // that means the identity on either side, are never ambiguous.
            let id_is_identity = fields.contains(IDENTITY_ALIAS_MARKER)
                || right_fields.contains(IDENTITY_ALIAS_MARKER);
            let ambiguous = fields
                .intersection(&right_fields)
                .filter(|field| !field.contains('.') && !field.starts_with("__cassie_ambiguous__."))
                .filter(|field| {
                    !(is_row_identity_column(field)
                        || (id_is_identity && is_legacy_id_column(field)))
                })
                .cloned()
                .collect::<Vec<_>>();
            for field in ambiguous {
                fields.insert(format!("__cassie_ambiguous__.{field}"));
            }
            fields.extend(right_fields);
            Ok(fields)
        }
    }
}

/// Marks a field set whose bare `id` is the internal-identity alias rather
/// than a declared column. Never a valid user identifier.
const IDENTITY_ALIAS_MARKER: &str = "__cassie_identity__.id";

/// The names a base table exposes: its declared fields, the reserved `_id`
/// row identity, and, only when the table declares no `id` of its own, the
/// legacy bare `id` alias for that identity. Derived relations never get
/// these implicitly; they expose only what they project.
pub(super) fn base_table_fields(
    qualifier: &str,
    schema: &crate::catalog::CollectionSchema,
) -> HashSet<String> {
    let mut names = schema
        .fields
        .iter()
        .map(|field| field.name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    names.push(ROW_IDENTITY_COLUMN.to_string());
    let alias_is_identity = !schema.declares_id();
    if alias_is_identity {
        names.push(LEGACY_ID_COLUMN.to_string());
    }
    let mut fields = qualified_fields(qualifier, names);
    if alias_is_identity {
        fields.insert(IDENTITY_ALIAS_MARKER.to_string());
    }
    fields
}

fn validate_graph_table_function(
    function: &FunctionCall,
    lateral_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let expected = match function.name.to_ascii_lowercase().as_str() {
        "pg_show_all_settings" | "pg_catalog.pg_show_all_settings" => 0,
        "graph_neighbors" => 6,
        "graph_expand" => 7,
        "graph_shortest_path" => 9,
        other => {
            return Err(CassieError::Planner(format!(
                "unsupported table function '{other}'"
            )))
        }
    };
    if function.args.len() != expected {
        return Err(CassieError::Planner(format!(
            "{} requires {} arguments",
            function.name, expected
        )));
    }
    for arg in &function.args {
        validate_expression(arg, lateral_fields, &HashSet::new(), false)?;
    }
    Ok(())
}

fn literal_string_arg(function: &FunctionCall, index: usize) -> Option<String> {
    match function.args.get(index)? {
        Expr::StringLiteral(value) => Some(value.clone()),
        _ => None,
    }
}

pub(super) fn graph_table_function_columns() -> Vec<(String, DataType)> {
    vec![
        ("depth".to_string(), DataType::BigInt),
        ("path_rank".to_string(), DataType::BigInt),
        ("cost".to_string(), DataType::Float),
        ("node_type".to_string(), DataType::Text),
        ("node_id".to_string(), DataType::Text),
        ("edge_id".to_string(), DataType::Text),
        ("edge_type".to_string(), DataType::Text),
        ("source_type".to_string(), DataType::Text),
        ("source_id".to_string(), DataType::Text),
        ("target_type".to_string(), DataType::Text),
        ("target_id".to_string(), DataType::Text),
        ("path_nodes".to_string(), DataType::Json),
        ("path_edges".to_string(), DataType::Json),
    ]
}

pub(super) fn table_function_columns(name: &str) -> Vec<(String, DataType)> {
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "pg_show_all_settings" | "pg_catalog.pg_show_all_settings"
    ) {
        return vec![
            ("name".to_string(), DataType::Text),
            ("setting".to_string(), DataType::Text),
            ("unit".to_string(), DataType::Text),
            ("category".to_string(), DataType::Text),
            ("short_desc".to_string(), DataType::Text),
            ("extra_desc".to_string(), DataType::Text),
            ("context".to_string(), DataType::Text),
            ("vartype".to_string(), DataType::Text),
            ("source".to_string(), DataType::Text),
            ("min_val".to_string(), DataType::Text),
            ("max_val".to_string(), DataType::Text),
            ("enumvals".to_string(), DataType::Text),
            ("boot_val".to_string(), DataType::Text),
            ("reset_val".to_string(), DataType::Text),
            ("sourcefile".to_string(), DataType::Text),
            ("sourceline".to_string(), DataType::BigInt),
            ("pending_restart".to_string(), DataType::Boolean),
        ];
    }
    graph_table_function_columns()
}
