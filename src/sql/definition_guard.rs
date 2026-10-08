//! Keeps newly selected transient operators out of durable definitions.

use super::ast::{
    BinaryOp, CteQuery, Expr, InsertConflictAction, InsertSource, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectStatement,
};
use crate::app::CassieError;

const DIAGNOSTIC: &str = "IS [NOT] DISTINCT FROM is not supported in persisted definitions; compatibility law is not selected";

pub(crate) fn expression(expr: &Expr) -> Result<(), CassieError> {
    if matches!(
        expr,
        Expr::Binary {
            op: BinaryOp::IsDistinctFrom | BinaryOp::IsNotDistinctFrom,
            ..
        }
    ) {
        return Err(CassieError::Unsupported(DIAGNOSTIC.into()));
    }
    if let Expr::Exists(query) = expr {
        statement(query)?;
    }
    expr.try_visit_children(expression)
}

pub(crate) fn expression_sql(sql: &str) -> Result<(), CassieError> {
    // Existing callers retain their own malformed-definition diagnostics.
    if let Ok(expr) = super::parser::parse_expression(sql) {
        expression(&expr)?;
    }
    Ok(())
}

pub(crate) fn query_sql(sql: &str) -> Result<(), CassieError> {
    if let Ok(query) = super::parse_statement(sql) {
        statement(&query)?;
    }
    Ok(())
}

pub(crate) fn statement(query: &ParsedStatement) -> Result<(), CassieError> {
    match &query.statement {
        QueryStatement::CreateView(definition) => query_sql(&definition.query)?,
        QueryStatement::CreateFunction(definition) => expression_sql(&definition.body)?,
        QueryStatement::CreateProcedure(definition) => query_sql(&definition.body)?,
        QueryStatement::CreateMaterializedProjection(definition) => query_sql(&definition.query)?,
        QueryStatement::CreateIndex(definition) => {
            for expr in definition.expressions.iter().chain(&definition.predicate) {
                expression(expr)?;
            }
        }
        QueryStatement::CreateRollup(definition) => {
            for expr in definition
                .bucket
                .args
                .iter()
                .chain(&definition.group_by)
                .chain(&definition.filter)
            {
                expression(expr)?;
            }
            items(&definition.aggregates)?;
        }
        QueryStatement::CallProcedure(call) => {
            for expr in &call.args {
                expression(expr)?;
            }
        }
        QueryStatement::Select(query) => select(query)?,
        QueryStatement::Explain(query) => statement(&query.statement)?,
        QueryStatement::Insert(insert) => {
            match &insert.source {
                InsertSource::Values(rows) => {
                    for expr in rows.iter().flatten() {
                        expression(expr)?;
                    }
                }
                InsertSource::Select(query) => select(query)?,
            }
            if let Some(conflict) = &insert.on_conflict {
                if let InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                } = &conflict.action
                {
                    for (_, expr) in assignments {
                        expression(expr)?;
                    }
                    if let Some(expr) = filter {
                        expression(expr)?;
                    }
                }
            }
            items(&insert.returning)?;
        }
        QueryStatement::Update(update) => {
            for (_, expr) in &update.assignments {
                expression(expr)?;
            }
            if let Some(expr) = &update.filter {
                expression(expr)?;
            }
            items(&update.returning)?;
        }
        QueryStatement::Delete(delete) => {
            if let Some(expr) = &delete.filter {
                expression(expr)?;
            }
            items(&delete.returning)?;
        }
        _ => {}
    }
    Ok(())
}

fn select(query: &SelectStatement) -> Result<(), CassieError> {
    source(&query.source)?;
    for cte in &query.ctes {
        match &cte.query {
            CteQuery::Simple(query) => statement(query)?,
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                statement(base)?;
                statement(recursive)?;
            }
        }
    }
    if let Some(set) = &query.set {
        select(&set.right)?;
    }
    items(&query.projection)?;
    for expr in query
        .filter
        .iter()
        .chain(&query.having)
        .chain(&query.group_by)
        .chain(&query.distinct_on)
        .chain(&query.limit)
        .chain(&query.offset)
    {
        expression(expr)?;
    }
    for order in &query.order {
        expression(&order.expr)?;
    }
    Ok(())
}

fn source(query: &QuerySource) -> Result<(), CassieError> {
    match query {
        QuerySource::Aliased { source: query, .. } => source(query)?,
        QuerySource::Subquery { select: query, .. } => select(query)?,
        QuerySource::Join {
            left, right, on, ..
        } => {
            source(left)?;
            source(right)?;
            expression(on)?;
        }
        QuerySource::TableFunction { function, .. } => {
            for expr in &function.args {
                expression(expr)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn items(projection: &[SelectItem]) -> Result<(), CassieError> {
    for item in projection {
        match item {
            SelectItem::Expr { expr, .. } => expression(expr)?,
            SelectItem::Function { function, .. } => {
                for expr in &function.args {
                    expression(expr)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter().chain(&function.partition_by) {
                    expression(expr)?;
                }
                for order in &function.order_by {
                    expression(&order.expr)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn encoded_expression(sql: &str) -> Result<(), CassieError> {
    if let Ok(expr) = serde_json::from_str::<Expr>(sql) {
        expression(&expr)?;
    }
    Ok(())
}

pub(crate) fn constraints(
    constraints: &[crate::catalog::FieldConstraint],
) -> Result<(), CassieError> {
    for constraint in constraints {
        if let Some(expression) = &constraint.default_expression {
            expression_sql(expression)?;
        }
    }
    Ok(())
}

pub(crate) fn index(metadata: &crate::catalog::IndexMeta) -> Result<(), CassieError> {
    for expression in metadata.expressions.iter().chain(&metadata.predicate) {
        encoded_expression(expression)?;
    }
    Ok(())
}

pub(crate) fn rollup(metadata: &crate::catalog::RollupMeta) -> Result<(), CassieError> {
    expression_sql(&metadata.bucket_expr)?;
    for expression in metadata
        .group_keys
        .iter()
        .chain(
            metadata
                .aggregates
                .iter()
                .map(|aggregate| &aggregate.expression),
        )
        .chain(&metadata.filter_expr)
    {
        expression_sql(expression)?;
    }
    Ok(())
}
