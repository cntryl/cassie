pub mod ast;
pub mod binder;
pub mod functions;
pub mod parser;

pub use ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CommonTableExpression, CreateSchemaStatement, CreateTableStatement, CreateViewStatement,
    DeleteStatement, DropSchemaStatement, DropTableStatement, DropViewStatement, FieldDefinition,
    InsertStatement, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
    UpdateStatement,
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

pub fn parameter_type_oids_with_catalog(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> Vec<i32> {
    let mut oids = parameter_type_oids(statement, provided);
    infer_parameter_type_oids_query(&statement.statement, catalog, &mut oids);
    oids
}

fn infer_parameter_type_oids_query(
    statement: &QueryStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    match statement {
        QueryStatement::Insert(statement) => {
            infer_insert_parameter_type_oids(statement, catalog, oids)
        }
        QueryStatement::Explain(statement) => {
            infer_parameter_type_oids_query(&statement.statement.statement, catalog, oids);
        }
        _ => {}
    }
}

fn infer_insert_parameter_type_oids(
    statement: &ast::InsertStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    if oids.len() <= 1 {
        return;
    }
    let ast::InsertSource::Values(rows) = &statement.source else {
        return;
    };
    let Some(schema) = catalog.get_schema(&statement.table) else {
        return;
    };
    let fields = if statement.columns.is_empty() {
        schema.fields
    } else {
        statement
            .columns
            .iter()
            .filter_map(|column| {
                schema
                    .fields
                    .iter()
                    .find(|field| field.name.eq_ignore_ascii_case(column))
                    .cloned()
            })
            .collect::<Vec<_>>()
    };
    for row in rows {
        for (expr, field) in row.iter().zip(fields.iter()) {
            if let ast::Expr::Param(index) = expr {
                if let Some(oid) = oids.get_mut(*index) {
                    if *oid == UNKNOWN_PARAMETER_TYPE_OID {
                        *oid = field.data_type.type_oid() as i32;
                    }
                }
            }
        }
    }
}

fn parameter_count_query(statement: &QueryStatement) -> usize {
    match statement {
        QueryStatement::Explain(statement) => parameter_count(&statement.statement),
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
        QueryStatement::CreateRollup(_) => 0,
        QueryStatement::RefreshRollup(_) => 0,
        QueryStatement::DropRollup(_) => 0,
        QueryStatement::CreateMaterializedProjection(_) => 0,
        QueryStatement::RefreshMaterializedProjection(_) => 0,
        QueryStatement::DropMaterializedProjection(_) => 0,
        QueryStatement::AlterMaterializedProjection(_) => 0,
        QueryStatement::DropMaterializedProjectionVersion(_) => 0,
        QueryStatement::VerifyProjection(_) => 0,
        QueryStatement::DiffProjection(_) => 0,
        QueryStatement::CompareProjection(_) => 0,
        QueryStatement::PlanRepairProjection(_) => 0,
        QueryStatement::RepairProjection(_) => 0,
        QueryStatement::CreateRetentionPolicy(_) => 0,
        QueryStatement::AlterRetentionPolicy(_) => 0,
        QueryStatement::DropRetentionPolicy(_) => 0,
        QueryStatement::EnforceRetentionPolicy(_) => 0,
        QueryStatement::CreateFunction(_) => 0,
        QueryStatement::DropFunction(_) => 0,
        QueryStatement::CreateProcedure(_) => 0,
        QueryStatement::DropProcedure(_) => 0,
        QueryStatement::DropView(_) => 0,
        QueryStatement::DropSchema(_) => 0,
        QueryStatement::AlterSchema(_) => 0,
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
        ast::Expr::Not { expr } => parameter_count_expr(expr),
        ast::Expr::Cast { expr, .. } => parameter_count_expr(expr),
        ast::Expr::Exists(statement) => parameter_count_query(&statement.statement),
        ast::Expr::Function(function) => parameter_count_function(function),
    }
}
