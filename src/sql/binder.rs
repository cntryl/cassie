use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::sql::ast::{
    CteQuery, Expr, FunctionCall, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
};

type CteScope = HashMap<String, Vec<String>>;

#[derive(Debug, Clone)]
pub struct BoundStatement {
    pub statement: ParsedStatement,
}

pub async fn bind(
    statement: ParsedStatement,
    catalog: &Catalog,
) -> Result<BoundStatement, CassieError> {
    let statement = bind_statement(statement, catalog, &HashMap::new()).await?;
    Ok(BoundStatement { statement })
}

fn unwrap_select(statement: ParsedStatement) -> Result<(String, SelectStatement), CassieError> {
    match statement.statement {
        QueryStatement::Select(select) => Ok((statement.raw_sql, select)),
    }
}

fn bind_statement<'a>(
    statement: ParsedStatement,
    catalog: &'a Catalog,
    outer_scope: &'a CteScope,
) -> Pin<Box<dyn Future<Output = Result<ParsedStatement, CassieError>> + Send + 'a>> {
    Box::pin(async move {
        let (raw_sql, mut select) = unwrap_select(statement)?;

        let mut scope = outer_scope.clone();
        let mut local_names = HashSet::new();

        let mut bound_ctes = Vec::with_capacity(select.ctes.len());
        for cte in select.ctes {
            let cte_name = cte.name.trim();
            if cte_name.is_empty() {
                return Err(CassieError::Planner("CTE name cannot be empty".into()));
            }
            let cte_name_lc = cte_name.to_ascii_lowercase();
            if !local_names.insert(cte_name_lc.clone()) {
                return Err(CassieError::Planner(format!("duplicate CTE name '{cte_name}'")));
            }

            let bound_query = match cte.query {
                CteQuery::Simple(next) => {
                    let next = Box::new(bind_statement(*next, catalog, &scope).await?);
                    CteQuery::Simple(next)
                }
                CteQuery::Recursive { base, recursive } => {
                    if cte.aliases.is_empty() {
                        return Err(CassieError::Planner(format!(
                            "recursive CTE '{cte_name}' requires column aliases"
                        )));
                    }

                    let mut recursive_scope = scope.clone();
                    recursive_scope.insert(cte_name_lc.clone(), cte.aliases.clone());

                    let base = Box::new(bind_statement(*base, catalog, &recursive_scope).await?);
                    let recursive =
                        Box::new(bind_statement(*recursive, catalog, &recursive_scope).await?);

                    if !references_cte_as_source(&recursive, cte_name) {
                        return Err(CassieError::Planner(format!(
                            "recursive CTE '{cte_name}' must reference itself in recursive term"
                        )));
                    }

                    CteQuery::Recursive { base, recursive }
                }
            };

            let visible_fields = cte_output_fields(&bound_query).await?;
            let fields = if cte.aliases.is_empty() {
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
                    .collect::<Vec<_>>()
            };

            scope.insert(cte_name_lc, fields);

            bound_ctes.push(crate::sql::ast::CommonTableExpression {
                name: cte.name,
                aliases: cte.aliases,
                query: bound_query,
            });
        }

        select.ctes = bound_ctes;

        let source_name = match &select.source {
            QuerySource::Collection(name) => name.to_string(),
            QuerySource::Cte(name) => name.to_string(),
        };
        let source_name_lc = source_name.to_ascii_lowercase();

        if scope.contains_key(&source_name_lc) {
            select.source = QuerySource::Cte(source_name);
        } else {
            if !catalog.exists(&source_name).await {
                return Err(CassieError::CollectionNotFound(source_name));
            }
            select.source = QuerySource::Collection(source_name);
        }

        let known_fields = source_fields(catalog, &select.source, &scope).await?;
        let projection_aliases = collect_projection_aliases(&select);

        let statement = ParsedStatement {
            raw_sql,
            statement: QueryStatement::Select(select),
        };

        validate_functions(&statement)?;
        validate_projection_references(select_projection(&statement), &known_fields)?;
        validate_expression_references(
            select_filter(&statement),
            &known_fields,
            &projection_aliases,
            false,
        )?;
        validate_order_by_references(select_order(&statement), &known_fields, &projection_aliases)?;

        Ok(statement)
    })
}

fn select_projection(statement: &ParsedStatement) -> &[SelectItem] {
    match &statement.statement {
        QueryStatement::Select(select) => &select.projection,
    }
}

fn select_filter(statement: &ParsedStatement) -> Option<&Expr> {
    match &statement.statement {
        QueryStatement::Select(select) => select.filter.as_ref(),
    }
}

fn select_order(statement: &ParsedStatement) -> std::slice::Iter<'_, crate::sql::ast::OrderExpr> {
    match &statement.statement {
        QueryStatement::Select(select) => select.order.iter(),
    }
}

fn references_cte_as_source(statement: &ParsedStatement, cte_name: &str) -> bool {
    let cte_name = cte_name.to_ascii_lowercase();
    match &statement.statement {
        QueryStatement::Select(select) => match &select.source {
            QuerySource::Cte(source) | QuerySource::Collection(source) => {
                source.to_ascii_lowercase() == cte_name
            }
        },
    }
}

async fn cte_output_fields(
    cte_query: &CteQuery,
) -> Result<Vec<String>, CassieError> {
    let query = match cte_query {
        CteQuery::Simple(statement) => statement,
        CteQuery::Recursive { base, .. } => base,
    };

    match &query.statement {
        QueryStatement::Select(select) => {
            if select.projection.iter().any(matches_wildcard) {
                return Ok(vec!["*".into()]);
            }
            Ok(projected_column_names(&select.projection))
        }
    }
}

fn projected_column_names(projection: &[SelectItem]) -> Vec<String> {
    projection
        .iter()
        .map(|item| match item {
            SelectItem::Wildcard => "*".to_string(),
            SelectItem::Column {
                name: _,
                alias: Some(alias),
                ..
            } => {
                alias.to_ascii_lowercase()
            }
            SelectItem::Column { name, alias: None } => name.to_ascii_lowercase(),
            SelectItem::Function { function, alias } => alias
                .as_deref()
                .unwrap_or(&function.name)
                .to_ascii_lowercase(),
        })
        .collect()
}

fn matches_wildcard(item: &SelectItem) -> bool {
    matches!(item, SelectItem::Wildcard)
}

async fn source_fields(
    catalog: &Catalog,
    source: &QuerySource,
    scope: &CteScope,
) -> Result<HashSet<String>, CassieError> {
    match source {
        QuerySource::Collection(name) => {
            let schema = catalog
                .get_schema(name)
                .await
                .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;

            Ok(schema
                .fields
                .iter()
                .map(|field| field.name.to_ascii_lowercase())
                .collect())
        }
        QuerySource::Cte(name) => scope
            .get(&name.to_ascii_lowercase())
            .cloned()
            .map(|fields| fields.into_iter().collect())
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone())),
    }
}

fn collect_projection_aliases(select: &SelectStatement) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for item in &select.projection {
        match item {
            SelectItem::Column {
                alias: Some(alias), ..
            }
            | SelectItem::Function {
                alias: Some(alias), ..
            } => {
                aliases.insert(alias.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    aliases
}

fn validate_functions(statement: &ParsedStatement) -> Result<(), CassieError> {
    let mut signatures = HashMap::new();
    for function in crate::sql::functions::registry() {
        signatures.insert(function.name.to_ascii_lowercase(), function.arity);
    }

    let mut seen = Vec::new();
    collect_functions(statement, &mut seen);

    for function in seen {
        let Some(arity) = signatures.get(&function.name.to_ascii_lowercase()) else {
            return Err(CassieError::Planner(format!(
                "unsupported function '{}'",
                function.name
            )));
        };

        if function.args.len() != *arity {
            return Err(CassieError::Planner(format!(
                "function '{}' expects {} args, got {}",
                function.name,
                arity,
                function.args.len()
            )));
        }
    }

    Ok(())
}

fn validate_expression_references(
    expression: Option<&Expr>,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    let Some(expression) = expression else {
        return Ok(());
    };

    validate_expression(
        expression,
        known_fields,
        projection_aliases,
        allow_projection_alias,
    )
}

fn validate_order_by_references(
    order: std::slice::Iter<'_, crate::sql::ast::OrderExpr>,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
) -> Result<(), CassieError> {
    for item in order {
        validate_expression(&item.expr, known_fields, projection_aliases, true)?;
    }
    Ok(())
}

fn validate_projection_references(
    projection: &[crate::sql::ast::SelectItem],
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    for item in projection {
        match item {
            crate::sql::ast::SelectItem::Wildcard => {}
            crate::sql::ast::SelectItem::Column { name, .. } => {
                validate_expression(
                    &Expr::Column(name.clone()),
                    known_fields,
                    &HashSet::new(),
                    false,
                )?;
            }
            crate::sql::ast::SelectItem::Function { function, .. } => {
                for arg in &function.args {
                    validate_expression(arg, known_fields, &HashSet::new(), false)?;
                }
            }
        }
    }

    Ok(())
}

fn validate_expression(
    expr: &Expr,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => {
            let name = name.to_ascii_lowercase();
            if known_fields.contains("*") || name == "id" || known_fields.contains(&name) {
                return Ok(());
            }

            if allow_projection_alias && projection_aliases.contains(&name) {
                return Ok(());
            }

            Err(CassieError::Planner(format!(
                "unresolvable column reference '{}'; known fields or aliases required",
                name
            )))
        }
        Expr::Binary { left, right, .. } => {
            validate_expression(
                left,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            validate_expression(
                right,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )
        }
        Expr::Function(function) => {
            for arg in &function.args {
                validate_expression(
                    arg,
                    known_fields,
                    projection_aliases,
                    allow_projection_alias,
                )?;
            }

            Ok(())
        }
        Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => Ok(()),
    }
}

fn collect_functions(statement: &ParsedStatement, out: &mut Vec<FunctionCall>) {
    let QueryStatement::Select(select) = &statement.statement;
    for item in &select.projection {
        collect_item(item, out);
    }
    if let Some(expr) = &select.filter {
        collect_expr(expr, out);
    }
    for order in &select.order {
        collect_expr(&order.expr, out);
    }
    for cte in &select.ctes {
        match &cte.query {
            CteQuery::Simple(statement) => {
                collect_functions(statement, out);
            }
            CteQuery::Recursive { base, recursive } => {
                collect_functions(base, out);
                collect_functions(recursive, out);
            }
        }
    }
}

fn collect_item(item: &SelectItem, out: &mut Vec<FunctionCall>) {
    if let SelectItem::Function { function, .. } = item {
        out.push(function.clone());
        for arg in &function.args {
            collect_expr(arg, out);
        }
    }
}

fn collect_expr(expr: &Expr, out: &mut Vec<FunctionCall>) {
    if let Expr::Function(function) = expr {
        out.push(function.clone());
        for arg in &function.args {
            collect_expr(arg, out);
        }
    }

    if let Expr::Binary { left, right, .. } = expr {
        collect_expr(left, out);
        collect_expr(right, out);
    }
}
