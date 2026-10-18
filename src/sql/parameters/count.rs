//! Counts existing statement parameter slots without type resolution.

use crate::sql::{ast, ParsedStatement, QueryStatement};

#[must_use]
pub fn parameter_count(statement: &ParsedStatement) -> usize {
    parameter_count_query(&statement.statement)
}

fn parameter_count_query(statement: &QueryStatement) -> usize {
    match statement {
        QueryStatement::Explain(statement) => parameter_count(&statement.statement),
        QueryStatement::Select(statement) => parameter_count_select(statement),
        QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Copy(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::CreateTable(_)
        | QueryStatement::CreateGraph(_)
        | QueryStatement::DropTable(_)
        | QueryStatement::AlterTable(_)
        | QueryStatement::CreateSequence(_)
        | QueryStatement::AlterSequence(_)
        | QueryStatement::DropSequence(_)
        | QueryStatement::CreateDatabase(_)
        | QueryStatement::DropDatabase(_)
        | QueryStatement::CreateSchema(_)
        | QueryStatement::CreateView(_)
        | QueryStatement::CreateRole(_)
        | QueryStatement::AlterRole(_)
        | QueryStatement::DropRole(_)
        | QueryStatement::GrantDatabaseConnect(_)
        | QueryStatement::RevokeDatabaseConnect(_)
        | QueryStatement::CreateIndex(_)
        | QueryStatement::DropIndex(_)
        | QueryStatement::CreateRollup(_)
        | QueryStatement::RefreshRollup(_)
        | QueryStatement::DropRollup(_)
        | QueryStatement::CreateMaterializedProjection(_)
        | QueryStatement::RefreshMaterializedProjection(_)
        | QueryStatement::DropMaterializedProjection(_)
        | QueryStatement::AlterMaterializedProjection(_)
        | QueryStatement::DropMaterializedProjectionVersion(_)
        | QueryStatement::VerifyProjection(_)
        | QueryStatement::DiffProjection(_)
        | QueryStatement::CompareProjection(_)
        | QueryStatement::PlanRepairProjection(_)
        | QueryStatement::RepairProjection(_)
        | QueryStatement::CreateRetentionPolicy(_)
        | QueryStatement::AlterRetentionPolicy(_)
        | QueryStatement::DropRetentionPolicy(_)
        | QueryStatement::EnforceRetentionPolicy(_)
        | QueryStatement::CreateFunction(_)
        | QueryStatement::DropFunction(_)
        | QueryStatement::CreateProcedure(_)
        | QueryStatement::DropProcedure(_)
        | QueryStatement::DropView(_)
        | QueryStatement::DropSchema(_)
        | QueryStatement::AlterSchema(_) => 0,
        QueryStatement::Insert(statement) => parameter_count_insert(statement),
        QueryStatement::Update(statement) => parameter_count_update(statement),
        QueryStatement::Delete(statement) => parameter_count_delete(statement),
        QueryStatement::CallProcedure(statement) => statement
            .args
            .iter()
            .map(parameter_count_expr)
            .max()
            .unwrap_or(0),
    }
}

fn parameter_count_select(statement: &ast::SelectStatement) -> usize {
    let mut count = parameter_count_query_source(&statement.source);
    for cte in &statement.ctes {
        count = count.max(parameter_count_cte_query(&cte.query));
    }
    for item in &statement.projection {
        count = count.max(parameter_count_select_item(item));
    }
    if let Some(filter) = &statement.filter {
        count = count.max(parameter_count_expr(filter));
    }
    for expr in &statement.distinct_on {
        count = count.max(parameter_count_expr(expr));
    }
    for expr in &statement.group_by {
        count = count.max(parameter_count_expr(expr));
    }
    if let Some(having) = &statement.having {
        count = count.max(parameter_count_expr(having));
    }
    for order in &statement.order {
        count = count.max(parameter_count_expr(&order.expr));
    }
    for bound in statement.limit.iter().chain(&statement.offset) {
        count = count.max(parameter_count_expr(bound));
    }
    if let Some(set) = &statement.set {
        count = count.max(parameter_count_select(set.right.as_ref()));
    }
    count
}

fn parameter_count_cte_query(query: &ast::CteQuery) -> usize {
    match query {
        ast::CteQuery::Simple(statement) => parameter_count_query(&statement.statement),
        ast::CteQuery::Recursive {
            base, recursive, ..
        } => {
            parameter_count_query(&base.statement).max(parameter_count_query(&recursive.statement))
        }
    }
}

fn parameter_count_query_source(source: &ast::QuerySource) -> usize {
    match source {
        ast::QuerySource::Values { rows } => rows
            .iter()
            .flatten()
            .map(parameter_count_expr)
            .max()
            .unwrap_or(0),

        ast::QuerySource::Aliased { source, .. } => parameter_count_query_source(source),
        ast::QuerySource::Collection(_)
        | ast::QuerySource::Cte(_)
        | ast::QuerySource::SingleRow => 0,
        ast::QuerySource::TableFunction { function, .. } => parameter_count_function(function),
        ast::QuerySource::Subquery { select, .. } => parameter_count_select(select),
        ast::QuerySource::Join {
            left, right, on, ..
        } => parameter_count_query_source(left)
            .max(parameter_count_query_source(right))
            .max(parameter_count_expr(on)),
    }
}

fn parameter_count_select_item(item: &ast::SelectItem) -> usize {
    match item {
        ast::SelectItem::Wildcard | ast::SelectItem::Column { .. } => 0,
        ast::SelectItem::Function { function, .. } => parameter_count_function(function),
        ast::SelectItem::Expr { expr, .. } => parameter_count_expr(expr),
        ast::SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .map(parameter_count_expr)
            .chain(function.partition_by.iter().map(parameter_count_expr))
            .chain(
                function
                    .order_by
                    .iter()
                    .map(|order| parameter_count_expr(&order.expr)),
            )
            .max()
            .unwrap_or(0),
    }
}

fn parameter_count_insert(statement: &ast::InsertStatement) -> usize {
    let mut count = 0;
    if let ast::InsertSource::Values(rows) = &statement.source {
        for row in rows {
            for value in row {
                count = count.max(parameter_count_expr(value));
            }
        }
    }
    if let ast::InsertSource::Select(select) = &statement.source {
        count = count.max(parameter_count_select(select));
    }
    if let Some(on_conflict) = &statement.on_conflict {
        if let ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
        } = &on_conflict.action
        {
            for (_, expr) in assignments {
                count = count.max(parameter_count_expr(expr));
            }
            if let Some(filter) = filter {
                count = count.max(parameter_count_expr(filter));
            }
        }
    }
    for item in &statement.returning {
        count = count.max(parameter_count_select_item(item));
    }
    count
}

fn parameter_count_update(statement: &ast::UpdateStatement) -> usize {
    let mut count = 0;
    for (_, expr) in &statement.assignments {
        count = count.max(parameter_count_expr(expr));
    }
    if let Some(filter) = &statement.filter {
        count = count.max(parameter_count_expr(filter));
    }
    for item in &statement.returning {
        count = count.max(parameter_count_select_item(item));
    }
    count
}

fn parameter_count_delete(statement: &ast::DeleteStatement) -> usize {
    let mut count = 0;
    if let Some(filter) = &statement.filter {
        count = count.max(parameter_count_expr(filter));
    }
    for item in &statement.returning {
        count = count.max(parameter_count_select_item(item));
    }
    count
}

fn parameter_count_function(function: &ast::FunctionCall) -> usize {
    function
        .args
        .iter()
        .map(parameter_count_expr)
        .max()
        .unwrap_or(0)
}

fn parameter_count_expr(expr: &ast::Expr) -> usize {
    match expr {
        ast::Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            let mut count = operand
                .as_ref()
                .map_or(0, |expr| parameter_count_expr(expr));
            for (when, then) in branches {
                count = count
                    .max(parameter_count_expr(when))
                    .max(parameter_count_expr(then));
            }
            count.max(
                else_expr
                    .as_ref()
                    .map_or(0, |expr| parameter_count_expr(expr)),
            )
        }
        ast::Expr::Column(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::IntegerLiteral(_)
        | ast::Expr::NumericLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => 0,
        ast::Expr::Param(index) => index + 1,
        ast::Expr::RowCompare(comparison) => comparison
            .left
            .iter()
            .chain(&comparison.right)
            .map(parameter_count_expr)
            .max()
            .unwrap_or(0),
        ast::Expr::Binary { left, right, .. } => {
            parameter_count_expr(left).max(parameter_count_expr(right))
        }
        ast::Expr::IsNull { expr, .. }
        | ast::Expr::TruthTest { expr, .. }
        | ast::Expr::Not { expr }
        | ast::Expr::Cast { expr, .. }
        | ast::Expr::UnaryNumeric { expr, .. }
        | ast::Expr::IntegerNegate { expr, .. } => parameter_count_expr(expr),
        ast::Expr::InList { expr, values, .. } => values
            .iter()
            .fold(parameter_count_expr(expr), |count, value| {
                count.max(parameter_count_expr(value))
            }),
        ast::Expr::Between {
            expr, low, high, ..
        }
        | ast::Expr::Like {
            expr,
            pattern: low,
            escape: high,
        } => parameter_count_expr(expr)
            .max(parameter_count_expr(low))
            .max(parameter_count_expr(high)),
        ast::Expr::Exists(statement) => parameter_count_query(&statement.statement),
        ast::Expr::Function(function) => parameter_count_function(function),
    }
}
