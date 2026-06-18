pub mod ast;
pub mod binder;
pub mod functions;
pub mod parser;

pub use ast::{
    AlterTableOperation, AlterTableStatement, CommonTableExpression, CreateSchemaStatement,
    CreateTableStatement, CreateViewStatement, DeleteStatement, DropTableStatement,
    DropViewStatement, FieldDefinition, InsertStatement, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectStatement, UpdateStatement,
};
pub use binder::{bind, BoundStatement};
pub use functions::registry;
pub use parser::{parse_statement, SqlError};

const UNKNOWN_PARAMETER_TYPE_OID: i32 = 705;

pub fn parameter_count(statement: &ParsedStatement) -> usize {
    parameter_count_query(&statement.statement)
}

pub fn parameter_type_oids(statement: &ParsedStatement, provided: &[i32]) -> Vec<i32> {
    let count = parameter_count(statement);
    let mut oids = provided.iter().copied().take(count).collect::<Vec<_>>();
    if oids.len() < count {
        oids.extend(std::iter::repeat_n(
            UNKNOWN_PARAMETER_TYPE_OID,
            count - oids.len(),
        ));
    }
    oids
}

fn parameter_count_query(statement: &QueryStatement) -> usize {
    match statement {
        QueryStatement::Select(statement) => parameter_count_select(statement),
        QueryStatement::Show(_) => 0,
        QueryStatement::Set(_) => 0,
        QueryStatement::Insert(statement) => parameter_count_insert(statement),
        QueryStatement::Update(statement) => parameter_count_update(statement),
        QueryStatement::Delete(statement) => parameter_count_delete(statement),
        QueryStatement::Transaction(_) => 0,
        QueryStatement::CreateTable(_) => 0,
        QueryStatement::DropTable(_) => 0,
        QueryStatement::AlterTable(_) => 0,
        QueryStatement::CreateSchema(_) => 0,
        QueryStatement::CreateView(_) => 0,
        QueryStatement::CreateRole(_) => 0,
        QueryStatement::AlterRole(_) => 0,
        QueryStatement::DropRole(_) => 0,
        QueryStatement::CreateIndex(_) => 0,
        QueryStatement::DropIndex(_) => 0,
        QueryStatement::CreateFunction(_) => 0,
        QueryStatement::DropFunction(_) => 0,
        QueryStatement::CreateProcedure(_) => 0,
        QueryStatement::DropProcedure(_) => 0,
        QueryStatement::DropView(_) => 0,
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
    for expr in &statement.group_by {
        count = count.max(parameter_count_expr(expr));
    }
    if let Some(having) = &statement.having {
        count = count.max(parameter_count_expr(having));
    }
    for order in &statement.order {
        count = count.max(parameter_count_expr(&order.expr));
    }
    if let Some(set) = &statement.set {
        count = count.max(parameter_count_select(set.right.as_ref()));
    }
    count
}

fn parameter_count_cte_query(query: &ast::CteQuery) -> usize {
    match query {
        ast::CteQuery::Simple(statement) => parameter_count_query(&statement.statement),
        ast::CteQuery::Recursive { base, recursive } => {
            parameter_count_query(&base.statement).max(parameter_count_query(&recursive.statement))
        }
    }
}

fn parameter_count_query_source(source: &ast::QuerySource) -> usize {
    match source {
        ast::QuerySource::Collection(_)
        | ast::QuerySource::Cte(_)
        | ast::QuerySource::SingleRow => 0,
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
    }
}

fn parameter_count_insert(statement: &ast::InsertStatement) -> usize {
    let mut count = 0;
    if let ast::InsertSource::Values(values) = &statement.source {
        for value in values {
            count = count.max(parameter_count_expr(value));
        }
    }
    if let ast::InsertSource::Select(select) = &statement.source {
        count = count.max(parameter_count_select(select));
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
        ast::Expr::Column(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => 0,
        ast::Expr::Param(index) => index + 1,
        ast::Expr::Binary { left, right, .. } => {
            parameter_count_expr(left).max(parameter_count_expr(right))
        }
        ast::Expr::IsNull { expr, .. } => parameter_count_expr(expr),
        ast::Expr::InList { expr, values, .. } => values
            .iter()
            .fold(parameter_count_expr(expr), |count, value| {
                count.max(parameter_count_expr(value))
            }),
        ast::Expr::Between {
            expr, low, high, ..
        } => parameter_count_expr(expr)
            .max(parameter_count_expr(low))
            .max(parameter_count_expr(high)),
        ast::Expr::Cast { expr, .. } => parameter_count_expr(expr),
        ast::Expr::Exists(statement) => parameter_count_query(&statement.statement),
        ast::Expr::Function(function) => parameter_count_function(function),
    }
}
