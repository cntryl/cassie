//! Collect scalar calls while preserving FROM-only generated function boundaries.
use super::{
    CteQuery, Expr, FunctionCall, QuerySource, QueryStatement, SelectItem, SelectStatement,
};

pub(crate) fn collect_functions(statement: &SelectStatement, out: &mut Vec<FunctionCall>) {
    collect_source_functions(&statement.source, out);
    for item in &statement.projection {
        collect_item(item, out);
    }
    if let Some(expr) = &statement.filter {
        collect_expr(expr, out);
    }
    if let Some(expr) = &statement.having {
        collect_expr(expr, out);
    }
    for expr in &statement.distinct_on {
        collect_expr(expr, out);
    }
    for expr in &statement.group_by {
        collect_expr(expr, out);
    }
    for order in &statement.order {
        collect_expr(&order.expr, out);
    }
    if let Some(set) = &statement.set {
        collect_functions(&set.right, out);
    }
    for cte in &statement.ctes {
        match &cte.query {
            CteQuery::Simple(statement) => {
                if let QueryStatement::Select(select) = &statement.statement {
                    collect_functions(select, out);
                }
            }
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                if let QueryStatement::Select(select) = &base.statement {
                    collect_functions(select, out);
                }
                if let QueryStatement::Select(select) = &recursive.statement {
                    collect_functions(select, out);
                }
            }
        }
    }
}

fn collect_source_functions(source: &QuerySource, out: &mut Vec<FunctionCall>) {
    match source {
        QuerySource::Values { rows } => {
            for expr in rows.iter().flatten() {
                collect_expr(expr, out);
            }
        }

        QuerySource::Aliased { source, .. } => collect_source_functions(source, out),
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            collect_source_functions(source, out);
            collect_expr(rate, out);
            if let Some(seed) = repeatable {
                collect_expr(seed, out);
            }
        }
        QuerySource::TableFunction { function, .. } => {
            if !crate::sql::functions::is_catalog_table(&function.name)
                && !crate::sql::table_sources::is_generated(&function.name)
            {
                out.push(function.clone());
            }
            for arg in function.expressions() {
                collect_expr(arg, out);
            }
        }
        QuerySource::Subquery { select, .. } => collect_functions(select, out),
        QuerySource::Join {
            left, right, on, ..
        } => {
            collect_source_functions(left, out);
            collect_source_functions(right, out);
            collect_expr(on, out);
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => {}
    }
}

pub(crate) fn collect_item(item: &SelectItem, out: &mut Vec<FunctionCall>) {
    match item {
        SelectItem::Function { function, .. } => {
            out.push(function.clone());
            for arg in function.expressions() {
                collect_expr(arg, out);
            }
        }
        SelectItem::WindowFunction { function, .. } => {
            for arg in &function.args {
                collect_expr(arg, out);
            }
            for expr in &function.partition_by {
                collect_expr(expr, out);
            }
            for order in &function.order_by {
                collect_expr(&order.expr, out);
            }
        }
        SelectItem::Expr { expr, .. } => {
            collect_expr(expr, out);
        }
        SelectItem::Wildcard | SelectItem::Column { .. } => {}
    }
}

pub(crate) fn collect_expr(expr: &Expr, out: &mut Vec<FunctionCall>) {
    if let Expr::RowCompare(comparison) = expr {
        for field in comparison.left.iter().chain(&comparison.right) {
            collect_expr(field, out);
        }
    }
    if let Expr::Function(function) = expr {
        out.push((**function).clone());
        for arg in function.expressions() {
            collect_expr(arg, out);
        }
    }
    if let Expr::Binary { left, right, .. } = expr {
        collect_expr(left, out);
        collect_expr(right, out);
    }
    if let Expr::IsNull { expr, .. } | Expr::TruthTest { expr, .. } = expr {
        collect_expr(expr, out);
    }
    if let Expr::InList { expr, values, .. } = expr {
        collect_expr(expr, out);
        for value in values {
            collect_expr(value, out);
        }
    }
    if let Expr::Between {
        expr, low, high, ..
    }
    | Expr::Like {
        expr,
        pattern: low,
        escape: high,
        ..
    } = expr
    {
        collect_expr(expr, out);
        collect_expr(low, out);
        collect_expr(high, out);
    }
    if let Expr::Cast { expr, .. }
    | Expr::Collate { expr, .. }
    | Expr::UnaryNumeric { expr, .. }
    | Expr::IntegerNegate { expr, .. } = expr
    {
        collect_expr(expr, out);
    }
    if let Expr::Not { expr } = expr {
        collect_expr(expr, out);
    }
}
