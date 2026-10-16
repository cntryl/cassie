//! Keeps newly selected transient operators out of durable definitions.

use super::ast::{
    BinaryOp, CteQuery, Expr, InsertConflictAction, InsertSource, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectStatement,
};
use crate::app::CassieError;

const DIAGNOSTIC: &str = "IS [NOT] DISTINCT FROM is not supported in persisted definitions; compatibility law is not selected";

pub(crate) fn expression(expr: &Expr) -> Result<(), CassieError> {
    if let Expr::Function(function) = expr {
        if crate::sql::functions::function(&function.name)
            .is_some_and(|metadata| !metadata.is_aggregate)
        {
            let modifier = if function.is_distinct() {
                Some("DISTINCT")
            } else if !function.order_by.is_empty() {
                Some("ORDER BY")
            } else if function.filter.is_some() {
                Some("FILTER")
            } else {
                None
            };
            if let Some(modifier) = modifier {
                return Err(CassieError::Execution(format!(
                    "{modifier} specified, but {} is not an aggregate function",
                    function.name.to_ascii_lowercase()
                )));
            }
        }
    }
    if matches!(expr, Expr::RowCompare(comparison) if comparison.op.null_safe()) {
        return Err(CassieError::Unsupported(DIAGNOSTIC.into()));
    }
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
    match super::parse_statement(sql) {
        Ok(query) => statement(&query),
        Err(error)
            if error.kind() == super::SqlErrorKind::Unsupported
                && error
                    .message()
                    .strip_prefix("unsupported feature: ")
                    .unwrap_or(error.message())
                    == DIAGNOSTIC =>
        {
            Err(CassieError::Unsupported(DIAGNOSTIC.into()))
        }
        // Other malformed/unsupported definitions retain their existing caller diagnostics.
        Err(_) => Ok(()),
    }
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
        QueryStatement::CreateRollup(definition) => create_rollup(definition)?,
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
        QuerySource::Values { rows } => rows.iter().flatten().try_for_each(expression)?,

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
            for expr in function.expressions() {
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
                for expr in function.expressions() {
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
        if let Ok(Expr::Function(function)) = super::parser::parse_expression(expression) {
            reject_rollup_modifiers(&function)?;
        }
    }
    Ok(())
}

pub(crate) fn create_rollup(
    definition: &super::ast::CreateRollupStatement,
) -> Result<(), CassieError> {
    for item in &definition.aggregates {
        if let SelectItem::Function { function, .. } = item {
            reject_rollup_modifiers(function)?;
        }
    }
    for expr in definition
        .bucket
        .args
        .iter()
        .chain(&definition.group_by)
        .chain(&definition.filter)
    {
        expression(expr)?;
    }
    items(&definition.aggregates)
}

fn reject_rollup_modifiers(function: &super::ast::FunctionCall) -> Result<(), CassieError> {
    if function.is_distinct() || function.filter.is_some() || !function.order_by.is_empty() {
        return Err(CassieError::Unsupported(
            "aggregate modifiers are not supported in incremental rollup definitions".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_reject_scalar_aggregate_modifiers_given_a_persisted_expression_when_definition_admission_runs(
    ) {
        // Arrange
        let cases = ["LOWER(label) FILTER (WHERE TRUE)", "LOWER(DISTINCT label)"];

        // Act
        for sql in cases {
            let expression =
                crate::sql::parser::parse_expression(sql).expect("function clause syntax");
            let error = super::expression(&expression)
                .expect_err("scalar modifiers cannot enter persisted definitions");

            // Assert
            assert_eq!(error.descriptor().sql_state, "42809");
        }
        let control =
            crate::sql::parser::parse_expression("LOWER(ALL label)").expect("valid ALL control");
        assert!(super::expression(&control).is_ok());
    }
}
