use super::*;

pub(super) fn plan_projection_query(
    cassie: &Cassie,
    query: &str,
    _user_functions: &HashMap<String, FunctionMeta>,
    context: &crate::sql::binder::BindingContext,
) -> Result<ProjectionBuildPlan, QueryError> {
    let parsed = crate::sql::parser::parse_statement(query)
        .map_err(|error| QueryError::General(error.to_string()))?;
    crate::sql::definition_guard::statement(&parsed).map_err(QueryError::Cassie)?;
    if crate::sql::parameter_count(&parsed) != 0 {
        return Err(QueryError::General(
            "materialized projection definitions cannot contain bind parameters".into(),
        ));
    }
    let bound = crate::sql::binder::bind_with_context(parsed, &cassie.catalog, context)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let QueryStatement::Select(select) = &bound.statement.statement else {
        return Err(QueryError::General(
            "materialized projection definition must be a SELECT".into(),
        ));
    };
    let schema =
        crate::sql::binder::infer_select_schema_with_context(select, &cassie.catalog, context)
            .map_err(|error| QueryError::General(error.to_string()))?;
    let source_collections = collect_source_collections(&select.source);
    let mut logical = crate::planner::logical::plan(&bound)
        .map_err(|error| QueryError::General(error.to_string()))?;
    crate::planner::logical::rewrite_reserved_id_references(&mut logical, &cassie.catalog);
    Ok(ProjectionBuildPlan {
        logical,
        select: select.as_ref().clone(),
        schema,
        source_collections,
    })
}

pub(super) fn projection_binding_context(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    projection_name: Option<&str>,
) -> crate::sql::binder::BindingContext {
    let default_database = cassie.default_database.clone();
    let default_path = vec![crate::catalog::DEFAULT_SCHEMA.to_string()];
    if let Some(session) = session {
        let database = session
            .current_database()
            .unwrap_or(default_database.as_str());
        return if cassie.database_catalog_enforced() {
            crate::sql::binder::BindingContext::scoped(
                database.to_string(),
                cassie.binding_search_path(session),
            )
        } else {
            crate::sql::binder::BindingContext::unscoped(
                database.to_string(),
                cassie.binding_search_path(session),
            )
        };
    }

    if let Some(relation) = projection_name.and_then(crate::catalog::RelationId::parse_canonical) {
        let mut path = vec![relation.schema.clone()];
        if !relation
            .schema
            .eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA)
        {
            path.push(crate::catalog::DEFAULT_SCHEMA.to_string());
        }
        return if cassie.database_catalog_enforced() {
            crate::sql::binder::BindingContext::scoped(relation.database, path)
        } else {
            crate::sql::binder::BindingContext::unscoped(relation.database, path)
        };
    }

    if cassie.database_catalog_enforced() {
        crate::sql::binder::BindingContext::scoped(default_database, default_path)
    } else {
        crate::sql::binder::BindingContext::unscoped(default_database, default_path)
    }
}

pub(super) fn reject_unsupported_sources(
    cassie: &Cassie,
    sources: &[String],
    require_source_collection: bool,
) -> Result<(), QueryError> {
    if require_source_collection && sources.is_empty() {
        return Err(QueryError::General(
            "materialized projection requires at least one source collection".into(),
        ));
    }
    for source in sources {
        if cassie.catalog.is_materialized_projection(source) {
            return Err(QueryError::General(
                "recursive materialized projections are not supported".into(),
            ));
        }
        if virtual_views::schema(source).is_some() {
            return Err(QueryError::General(
                "materialized projections over virtual catalog views are not supported".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn reject_nondeterministic_select(
    select: &crate::sql::ast::SelectStatement,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Result<(), QueryError> {
    for function in functions_in_select(select) {
        let name = function.to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "version"
                | "nextval"
                | "currval"
                | "setval"
                | "lastval"
                | "current_schema"
                | "current_database"
                | "current_user"
                | "session_user"
                | "current_role"
                | "search"
                | "search_score"
                | "hybrid_score"
                | "snippet"
        ) {
            return Err(QueryError::General(format!(
                "non-deterministic function '{function}' is not supported in materialized projections"
            )));
        }
        if user_functions
            .get(&name)
            .is_some_and(|function| function.volatility == catalog::Volatility::Volatile)
        {
            return Err(QueryError::General(format!(
                "volatile function '{function}' is not supported in materialized projections"
            )));
        }
    }
    Ok(())
}

fn collect_source_collections(source: &QuerySource) -> Vec<String> {
    let mut out = Vec::new();
    collect_source_collections_into(source, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_source_collections_into(source: &QuerySource, out: &mut Vec<String>) {
    match source {
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            collect_source_collections_into(source, out);
        }
        QuerySource::Collection(name) => out.push(name.to_string()),
        QuerySource::Subquery { select, .. } => {
            collect_source_collections_into(&select.source, out);
        }
        QuerySource::Join { left, right, .. } => {
            collect_source_collections_into(left, out);
            collect_source_collections_into(right, out);
        }
        QuerySource::Cte(_)
        | QuerySource::TableFunction { .. }
        | QuerySource::Values { .. }
        | QuerySource::SingleRow => {}
    }
}

fn functions_in_select(select: &crate::sql::ast::SelectStatement) -> Vec<String> {
    let mut out = Vec::new();
    for item in &select.projection {
        collect_functions_in_select_item(item, &mut out);
    }
    if let Some(filter) = &select.filter {
        collect_functions_in_expr(filter, &mut out);
    }
    for expr in &select.group_by {
        collect_functions_in_expr(expr, &mut out);
    }
    if let Some(having) = &select.having {
        collect_functions_in_expr(having, &mut out);
    }
    for order in &select.order {
        collect_functions_in_expr(&order.expr, &mut out);
    }
    if let Some(set) = &select.set {
        out.extend(functions_in_select(&set.right));
    }
    out
}

fn collect_functions_in_select_item(item: &SelectItem, out: &mut Vec<String>) {
    match item {
        SelectItem::Function { function, .. } => collect_functions_in_call(function, out),
        SelectItem::Expr { expr, .. } => collect_functions_in_expr(expr, out),
        SelectItem::WindowFunction { function, .. } => {
            out.push(function.name.clone());
            for arg in function.args.iter().chain(function.filter.as_deref()) {
                collect_functions_in_expr(arg, out);
            }
            for expr in &function.partition_by {
                collect_functions_in_expr(expr, out);
            }
            for order in &function.order_by {
                collect_functions_in_expr(&order.expr, out);
            }
        }
        SelectItem::Wildcard | SelectItem::Column { .. } => {}
    }
}

fn collect_functions_in_expr(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Function(function) => collect_functions_in_call(function, out),
        Expr::Exists(statement) => {
            if let QueryStatement::Select(select) = &statement.statement {
                out.extend(functions_in_select(select));
            }
        }
        Expr::Subquery(subquery) => {
            if let QueryStatement::Select(select) = &subquery.query.statement {
                out.extend(functions_in_select(select));
            }
            expr.for_each_child(|child| collect_functions_in_expr(child, out));
        }
        _ => expr.for_each_child(|child| collect_functions_in_expr(child, out)),
    }
}

fn collect_functions_in_call(function: &FunctionCall, out: &mut Vec<String>) {
    out.push(function.name.clone());
    for arg in function.expressions() {
        collect_functions_in_expr(arg, out);
    }
}
