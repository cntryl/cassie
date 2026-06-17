use std::collections::HashMap;

use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::sql::ast::{Expr, FunctionCall, ParsedStatement, QueryStatement, SelectItem};

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

    validate_functions(&statement)?;
    Ok(BoundStatement { statement })
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
