use std::collections::HashMap;

use crate::catalog::FieldMeta;
use crate::types::DataType;

pub mod ast;
pub mod binder;
pub mod functions;
pub mod parser;

pub use ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CatalogStatement, CatalogStatementRef, CommonTableExpression, CopyFormat, CopyStatement,
    CreateSchemaStatement, CreateTableStatement, CreateViewStatement, DeleteStatement,
    DropSchemaStatement, DropTableStatement, DropViewStatement, FieldDefinition, InsertStatement,
    ParsedStatement, ProjectionStatement, ProjectionStatementRef, QuerySource, QueryStatement,
    RetentionStatement, RetentionStatementRef, RuntimeStatement, RuntimeStatementRef, SelectItem,
    SelectStatement, StatementFamily, StatementRoute, StatementRouteRef, UpdateStatement,
};
pub use binder::{bind, BoundStatement};
pub use functions::registry;
pub use parser::{parse_statement, SqlError, SqlErrorKind};

const UNKNOWN_PARAMETER_TYPE_OID: i32 = 705;
type FieldTypeMap = HashMap<String, DataType>;

#[must_use]
pub fn parameter_count(statement: &ParsedStatement) -> usize {
    parameter_count_query(&statement.statement)
}

#[must_use]
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

#[must_use]
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
        QueryStatement::Select(statement) => {
            infer_select_parameter_type_oids(statement, catalog, oids);
        }
        QueryStatement::Insert(statement) => {
            infer_insert_parameter_type_oids(statement, catalog, oids);
        }
        QueryStatement::Update(statement) => {
            infer_update_parameter_type_oids(statement, catalog, oids);
        }
        QueryStatement::Delete(statement) => {
            infer_delete_parameter_type_oids(statement, catalog, oids);
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
    if oids.is_empty() {
        return;
    }
    let Some(schema) = catalog.get_schema(&statement.table) else {
        return;
    };
    let fields = if statement.columns.is_empty() {
        schema.fields.clone()
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
    let field_types = field_type_map(fields.iter());

    match &statement.source {
        ast::InsertSource::Values(rows) => {
            for row in rows {
                for (expr, field) in row.iter().zip(fields.iter()) {
                    infer_parameter_type_from_expected_expr(expr, &field.data_type, oids);
                    infer_parameter_type_oids_expr(expr, &field_types, catalog, oids);
                }
            }
        }
        ast::InsertSource::Select(select) => {
            infer_select_parameter_type_oids(select, catalog, oids);
        }
    }

    if let Some(conflict) = &statement.on_conflict {
        let schema_fields = field_type_map(schema.fields.iter());
        if let ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
        } = &conflict.action
        {
            infer_assignment_parameter_type_oids(assignments, &schema_fields, catalog, oids);
            if let Some(filter) = filter {
                infer_parameter_type_oids_expr(filter, &schema_fields, catalog, oids);
            }
        }
    }
}

fn infer_select_parameter_type_oids(
    statement: &ast::SelectStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    let field_types = source_field_type_map(&statement.source, catalog);
    for cte in &statement.ctes {
        match &cte.query {
            ast::CteQuery::Simple(statement) => {
                infer_parameter_type_oids_query(&statement.statement, catalog, oids);
            }
            ast::CteQuery::Recursive { base, recursive } => {
                infer_parameter_type_oids_query(&base.statement, catalog, oids);
                infer_parameter_type_oids_query(&recursive.statement, catalog, oids);
            }
        }
    }
    for item in &statement.projection {
        infer_select_item_parameter_type_oids(item, &field_types, catalog, oids);
    }
    if let Some(filter) = &statement.filter {
        infer_parameter_type_oids_expr(filter, &field_types, catalog, oids);
    }
    for expr in &statement.distinct_on {
        infer_parameter_type_oids_expr(expr, &field_types, catalog, oids);
    }
    for expr in &statement.group_by {
        infer_parameter_type_oids_expr(expr, &field_types, catalog, oids);
    }
    if let Some(having) = &statement.having {
        infer_parameter_type_oids_expr(having, &field_types, catalog, oids);
    }
    for order in &statement.order {
        infer_parameter_type_oids_expr(&order.expr, &field_types, catalog, oids);
    }
    if let Some(set) = &statement.set {
        infer_select_parameter_type_oids(&set.right, catalog, oids);
    }
}

fn infer_update_parameter_type_oids(
    statement: &ast::UpdateStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    let Some(schema) = catalog.get_schema(&statement.table) else {
        return;
    };
    let field_types = field_type_map(schema.fields.iter());
    infer_assignment_parameter_type_oids(&statement.assignments, &field_types, catalog, oids);
    if let Some(filter) = &statement.filter {
        infer_parameter_type_oids_expr(filter, &field_types, catalog, oids);
    }
    for item in &statement.returning {
        infer_select_item_parameter_type_oids(item, &field_types, catalog, oids);
    }
}

fn infer_delete_parameter_type_oids(
    statement: &ast::DeleteStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    let Some(schema) = catalog.get_schema(&statement.table) else {
        return;
    };
    let field_types = field_type_map(schema.fields.iter());
    if let Some(filter) = &statement.filter {
        infer_parameter_type_oids_expr(filter, &field_types, catalog, oids);
    }
    for item in &statement.returning {
        infer_select_item_parameter_type_oids(item, &field_types, catalog, oids);
    }
}

fn infer_assignment_parameter_type_oids(
    assignments: &[(String, ast::Expr)],
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    for (field, expr) in assignments {
        if let Some(data_type) = field_type_for_column(field_types, field) {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
        }
        infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
    }
}

fn infer_select_item_parameter_type_oids(
    item: &ast::SelectItem,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    match item {
        ast::SelectItem::Wildcard | ast::SelectItem::Column { .. } => {}
        ast::SelectItem::Function { function, .. } => {
            infer_function_parameter_type_oids(function, field_types, catalog, oids);
        }
        ast::SelectItem::Expr { expr, .. } => {
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::SelectItem::WindowFunction { function, .. } => {
            for arg in &function.args {
                infer_parameter_type_oids_expr(arg, field_types, catalog, oids);
            }
            for expr in &function.partition_by {
                infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
            }
            for order in &function.order_by {
                infer_parameter_type_oids_expr(&order.expr, field_types, catalog, oids);
            }
        }
    }
}

fn infer_parameter_type_oids_expr(
    expr: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    match expr {
        ast::Expr::Binary { left, right, .. } => {
            if let Some(data_type) = column_expr_type(left, field_types) {
                infer_parameter_type_from_expected_expr(right, data_type, oids);
            }
            if let Some(data_type) = column_expr_type(right, field_types) {
                infer_parameter_type_from_expected_expr(left, data_type, oids);
            }
            infer_parameter_type_oids_expr(left, field_types, catalog, oids);
            infer_parameter_type_oids_expr(right, field_types, catalog, oids);
        }
        ast::Expr::InList { expr, values, .. } => {
            if let Some(data_type) = column_expr_type(expr, field_types) {
                for value in values {
                    infer_parameter_type_from_expected_expr(value, data_type, oids);
                }
            }
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
            for value in values {
                infer_parameter_type_oids_expr(value, field_types, catalog, oids);
            }
        }
        ast::Expr::Between {
            expr, low, high, ..
        } => {
            if let Some(data_type) = column_expr_type(expr, field_types) {
                infer_parameter_type_from_expected_expr(low, data_type, oids);
                infer_parameter_type_from_expected_expr(high, data_type, oids);
            }
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
            infer_parameter_type_oids_expr(low, field_types, catalog, oids);
            infer_parameter_type_oids_expr(high, field_types, catalog, oids);
        }
        ast::Expr::IsNull { expr, .. } | ast::Expr::Not { expr } => {
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::Cast { expr, data_type } => {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::Exists(statement) => {
            infer_parameter_type_oids_query(&statement.statement, catalog, oids);
        }
        ast::Expr::Function(function) => {
            infer_function_parameter_type_oids(function, field_types, catalog, oids);
        }
        ast::Expr::Column(_)
        | ast::Expr::Param(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => {}
    }
}

fn infer_function_parameter_type_oids(
    function: &ast::FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut [i32],
) {
    for arg in &function.args {
        infer_parameter_type_oids_expr(arg, field_types, catalog, oids);
    }
}

fn infer_parameter_type_from_expected_expr(
    expr: &ast::Expr,
    data_type: &DataType,
    oids: &mut [i32],
) {
    match expr {
        ast::Expr::Param(index) => set_parameter_type_oid(oids, *index, data_type),
        ast::Expr::Cast { expr, data_type } => {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
        }
        _ => {}
    }
}

fn set_parameter_type_oid(oids: &mut [i32], index: usize, data_type: &DataType) {
    if let Some(oid) = oids.get_mut(index) {
        if *oid == UNKNOWN_PARAMETER_TYPE_OID {
            *oid = i32::try_from(data_type.type_oid()).unwrap_or(i32::MAX);
        }
    }
}

fn source_field_type_map(
    source: &ast::QuerySource,
    catalog: &crate::catalog::Catalog,
) -> FieldTypeMap {
    match source {
        ast::QuerySource::Collection(collection) => catalog
            .get_schema(collection)
            .map(|schema| field_type_map(schema.fields.iter()))
            .unwrap_or_default(),
        ast::QuerySource::Join { left, right, .. } => {
            let mut fields = source_field_type_map(left, catalog);
            fields.extend(source_field_type_map(right, catalog));
            fields
        }
        ast::QuerySource::Subquery { select, .. } => {
            let mut fields = source_field_type_map(&select.source, catalog);
            for cte in &select.ctes {
                if let ast::CteQuery::Simple(statement) = &cte.query {
                    infer_projected_field_types(&statement.statement, catalog, &mut fields);
                }
            }
            fields
        }
        ast::QuerySource::Cte(_)
        | ast::QuerySource::TableFunction { .. }
        | ast::QuerySource::SingleRow => FieldTypeMap::new(),
    }
}

fn infer_projected_field_types(
    statement: &QueryStatement,
    catalog: &crate::catalog::Catalog,
    fields: &mut FieldTypeMap,
) {
    if let QueryStatement::Select(select) = statement {
        fields.extend(source_field_type_map(&select.source, catalog));
    }
}

fn field_type_map<'a>(fields: impl IntoIterator<Item = &'a FieldMeta>) -> FieldTypeMap {
    fields
        .into_iter()
        .map(|field| (field.name.to_ascii_lowercase(), field.data_type.clone()))
        .collect()
}

fn column_expr_type<'a>(expr: &ast::Expr, field_types: &'a FieldTypeMap) -> Option<&'a DataType> {
    match expr {
        ast::Expr::Column(column) => field_type_for_column(field_types, column),
        ast::Expr::Cast { expr, .. } => column_expr_type(expr, field_types),
        _ => None,
    }
}

fn field_type_for_column<'a>(field_types: &'a FieldTypeMap, column: &str) -> Option<&'a DataType> {
    let column = column
        .trim_matches('"')
        .rsplit('.')
        .next()
        .unwrap_or(column)
        .trim_matches('"')
        .to_ascii_lowercase();
    field_types.get(&column)
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
        | QueryStatement::DropSequence(_)
        | QueryStatement::CreateDatabase(_)
        | QueryStatement::DropDatabase(_)
        | QueryStatement::CreateSchema(_)
        | QueryStatement::CreateView(_)
        | QueryStatement::CreateRole(_)
        | QueryStatement::AlterRole(_)
        | QueryStatement::DropRole(_)
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
        ast::Expr::Column(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => 0,
        ast::Expr::Param(index) => index + 1,
        ast::Expr::Binary { left, right, .. } => {
            parameter_count_expr(left).max(parameter_count_expr(right))
        }
        ast::Expr::IsNull { expr, .. } | ast::Expr::Not { expr } | ast::Expr::Cast { expr, .. } => {
            parameter_count_expr(expr)
        }
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
        ast::Expr::Exists(statement) => parameter_count_query(&statement.statement),
        ast::Expr::Function(function) => parameter_count_function(function),
    }
}
