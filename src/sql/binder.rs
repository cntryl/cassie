use std::collections::HashMap;

use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::sql::ast::{Expr, FunctionCall, ParsedStatement, QueryStatement, SelectItem};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct BoundStatement {
    pub statement: ParsedStatement,
}

pub async fn bind(
    statement: ParsedStatement,
    catalog: &Catalog,
) -> Result<BoundStatement, CassieError> {
    let QueryStatement::Select(select) = &statement.statement;
    if !catalog.exists(&select.collection).await {
        return Err(CassieError::CollectionNotFound(select.collection.clone()));
    }

    let projection_aliases = collect_projection_aliases(select);

    let schema = catalog
        .get_schema(&select.collection)
        .await
        .ok_or_else(|| CassieError::CollectionNotFound(select.collection.clone()))?;
    let known_fields = schema
        .fields
        .iter()
        .map(|field| field.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    validate_functions(&statement)?;
    validate_projection_references(&select.projection, &known_fields)?;
    validate_expression_references(
        select.filter.as_ref(),
        &known_fields,
        &projection_aliases,
        false,
    )?;
    validate_order_by_references(select.order.iter(), &known_fields, &projection_aliases)?;

    Ok(BoundStatement { statement })
}

fn collect_projection_aliases(select: &crate::sql::ast::SelectStatement) -> HashSet<String> {
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
            if name == "id" || known_fields.contains(&name) {
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
    match &statement.statement {
        QueryStatement::Select(select) => {
            for item in &select.projection {
                collect_item(item, out);
            }
            if let Some(expr) = &select.filter {
                collect_expr(expr, out);
            }
            for order in &select.order {
                collect_expr(&order.expr, out);
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
