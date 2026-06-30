use super::{
    bind_statement, collect_projection_aliases, mem, qualified_fields,
    recursive_cte_references_self, validate_distinct_on_order_prefix, validate_expression,
    validate_expression_references, validate_functions, validate_order_by_references,
    validate_projection_references, virtual_views, CassieError, Catalog, CteQuery, CteScope,
    DataType, Expr, FunctionCall, HashSet, QuerySource, QueryStatement, SelectItem, SelectSet,
    SelectStatement,
};

pub(super) fn bind_select(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
) -> Result<SelectStatement, CassieError> {
    bind_select_with_lateral_fields(select, catalog, outer_scope, &HashSet::new())
}

pub(super) fn bind_select_with_lateral_fields(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    lateral_fields: &HashSet<String>,
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

        let query = match cte.query {
            CteQuery::Simple(next) => {
                CteQuery::Simple(Box::new(bind_statement(*next, catalog, &scope)?))
            }
            CteQuery::Recursive { base, recursive } => {
                if cte.aliases.is_empty() {
                    return Err(CassieError::Planner(format!(
                        "recursive CTE '{cte_name}' requires column aliases"
                    )));
                }

                let mut recursive_scope = scope.clone();
                recursive_scope.insert(cte_name_lc.clone(), cte.aliases.clone());

                let bound_base = bind_statement(*base, catalog, &recursive_scope)?;
                let bound_recursive = bind_statement(*recursive, catalog, &recursive_scope)?;

                if !recursive_cte_references_self(&bound_recursive, cte_name) {
                    return Err(CassieError::Planner(format!(
                        "recursive CTE '{cte_name}' must reference itself in recursive term"
                    )));
                }

                CteQuery::Recursive {
                    base: Box::new(bound_base),
                    recursive: Box::new(bound_recursive),
                }
            }
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
        scope.insert(cte_name_lc, aliases);

        bound_ctes.push(crate::sql::ast::CommonTableExpression {
            name: cte.name,
            aliases: cte.aliases,
            query,
        });
    }

    let source = bind_query_source_with_lateral_fields(
        select.source.clone(),
        catalog,
        &scope,
        lateral_fields,
    )?;
    let mut known_fields = source_fields(catalog, &source, &scope)?;
    known_fields.extend(lateral_fields.iter().cloned());
    select.source = source;
    select.ctes = bound_ctes;

    let projection_aliases = collect_projection_aliases(&select);
    validate_bound_select_references(&select, &known_fields, &projection_aliases)?;

    if let Some(set) = set {
        let right = bind_select(*set.right, catalog, &scope)?;
        select.set = Some(Box::new(SelectSet {
            operator: set.operator,
            right: Box::new(right),
        }));
    }

    validate_functions(&select, catalog)?;

    Ok(select)
}

fn validate_bound_select_references(
    select: &SelectStatement,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
) -> Result<(), CassieError> {
    validate_projection_references(&select.projection, known_fields)?;
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
) -> Result<QuerySource, CassieError> {
    match source {
        QuerySource::Collection(name) => {
            let source_name_lc = name.to_ascii_lowercase();
            if scope.contains_key(&source_name_lc) {
                Ok(QuerySource::Cte(name))
            } else if catalog.relation_exists(&name) || virtual_views::schema(&name).is_some() {
                Ok(QuerySource::Collection(name))
            } else {
                Err(CassieError::CollectionNotFound(name))
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
            let select =
                bind_select_with_lateral_fields(*select, catalog, scope, visible_lateral_fields)?;
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
            let left =
                bind_query_source_with_lateral_fields(*left, catalog, scope, lateral_fields)?;
            let mut right_lateral_fields = lateral_fields.clone();
            right_lateral_fields.extend(source_fields(catalog, &left, scope)?);
            let right = bind_query_source_with_lateral_fields(
                *right,
                catalog,
                scope,
                &right_lateral_fields,
            )?;
            let joined = QuerySource::Join {
                left: Box::new(left),
                right: Box::new(right),
                kind,
                on: on.clone(),
            };
            let known_fields = source_fields(catalog, &joined, scope)?;
            validate_expression(&on, &known_fields, &HashSet::new(), false)?;
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
                Ok(qualified_fields(
                    name,
                    schema
                        .fields
                        .iter()
                        .map(|field| field.name.to_ascii_lowercase()),
                ))
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
            graph_table_function_columns()
                .into_iter()
                .map(|(name, _)| name),
        )),
        QuerySource::Subquery { alias, select, .. } => Ok(qualified_fields(
            alias,
            projected_column_names(&select.projection),
        )),
        QuerySource::Join { left, right, .. } => {
            let mut fields = source_fields(catalog, left, scope)?;
            fields.extend(source_fields(catalog, right, scope)?);
            Ok(fields)
        }
    }
}

fn validate_graph_table_function(
    function: &FunctionCall,
    lateral_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let expected = match function.name.to_ascii_lowercase().as_str() {
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
