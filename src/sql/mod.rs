use std::collections::HashMap;

use crate::catalog::FieldMeta;
use crate::types::DataType;

pub mod ast;
pub mod binder;
mod column_identifier;
pub mod functions;
pub(crate) mod pagination;
pub mod parser;
mod source_identity;
mod source_types;
pub(crate) use source_identity::physical_collection;
pub(crate) use source_types::source_field_type_map_with_ctes;

pub use ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CatalogStatement, CatalogStatementRef, CommonTableExpression, CopyFormat, CopyStatement,
    CreateSchemaStatement, CreateTableStatement, CreateViewStatement, DeleteStatement,
    DropSchemaStatement, DropTableStatement, DropViewStatement, FieldDefinition,
    IdentifierComponent, IdentifierPath, InsertStatement, ParsedStatement, ProjectionStatement,
    ProjectionStatementRef, QuerySource, QueryStatement, RetentionStatement, RetentionStatementRef,
    RuntimeStatement, RuntimeStatementRef, SelectItem, SelectStatement, StatementFamily,
    StatementRoute, StatementRouteRef, UpdateStatement,
};
pub use binder::{bind, BoundStatement};
pub(crate) use column_identifier::ColumnIdentifierPath;
pub use functions::registry;
pub use parser::{parse_statement, SqlError, SqlErrorKind};

const UNKNOWN_PARAMETER_TYPE_OID: i32 = 705;
pub(crate) type FieldTypeMap = HashMap<String, DataType>;

struct ParameterInference {
    oids: Vec<i32>,
    context_demands: Vec<u8>,
    record_context_demands: bool,
}

impl ParameterInference {
    fn new(oids: Vec<i32>) -> Self {
        Self {
            context_demands: vec![0; oids.len()],
            oids,
            record_context_demands: true,
        }
    }

    fn is_empty(&self) -> bool {
        self.oids.is_empty()
    }

    fn record_type_demand(&mut self, index: usize, data_type: &DataType) {
        if !self.record_context_demands {
            return;
        }
        let demand = match data_type {
            DataType::Boolean => 1,
            DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float => 2,
            _ => return,
        };
        if let Some(contexts) = self.context_demands.get_mut(index) {
            *contexts |= demand;
        }
    }

    fn record_expression_demand(&mut self, expr: &ast::Expr, data_type: &DataType) {
        if let ast::Expr::Param(index) = expr {
            self.record_type_demand(*index, data_type);
        }
    }

    fn without_context_demands(&mut self, infer: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(&mut self.record_context_demands, false);
        infer(self);
        self.record_context_demands = previous;
    }
}

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
    infer_parameter_types_with_catalog(statement, provided, catalog).oids
}

fn infer_parameter_types_with_catalog(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> ParameterInference {
    let mut inference = ParameterInference::new(parameter_type_oids(statement, provided));
    infer_parameter_type_oids_query(&statement.statement, catalog, &mut inference);
    inference
}

/// Returns a planner error message when a client-declared parameter type
/// cannot be compared with, or assigned to, the `BOOLEAN` operand it meets.
///
/// Literals of other families are rejected against `BOOLEAN` operands by
/// operand-family validation; a declared parameter type is held to the same
/// rule, as PostgreSQL does, instead of being compared by truthiness.
/// Explicit CAST result types do not create implicit Boolean input demands.
#[must_use]
pub fn declared_parameter_type_conflict(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> Option<String> {
    let boolean_oid = i32::try_from(DataType::Boolean.type_oid()).ok()?;
    let inference = infer_parameter_types_with_catalog(statement, &[], catalog);
    if let Some(index) = inference
        .context_demands
        .iter()
        .position(|demand| *demand == 3)
    {
        return Some(format!(
            "parameter ${} has incompatible boolean and numeric contexts",
            index + 1
        ));
    }
    provided
        .iter()
        .zip(inference.context_demands)
        .find_map(|(declared, demand)| {
            if demand & 1 == 0
                || matches!(*declared, 0 | UNKNOWN_PARAMETER_TYPE_OID)
                || *declared == boolean_oid
            {
                return None;
            }
            let family = match *declared {
                20 | 21 | 23 | 700 | 701 | 1700 => "numeric",
                _ => "text",
            };
            Some(format!(
                "incompatible comparison operands: boolean and {family}"
            ))
        })
}

fn infer_parameter_type_oids_query(
    statement: &QueryStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
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
    oids: &mut ParameterInference,
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
                    .find(|field| {
                        crate::sql::ColumnIdentifierPath::parse(column)
                            .is_ok_and(|reference| reference.matches_field_name(&field.name))
                    })
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
            infer_insert_select_boolean_parameter_type_oids(select, &fields, oids);
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
                infer_parameter_type_from_expected_expr(filter, &DataType::Boolean, oids);
                infer_parameter_type_oids_expr(filter, &schema_fields, catalog, oids);
            }
        }
    }
}

fn infer_insert_select_boolean_parameter_type_oids(
    select: &ast::SelectStatement,
    fields: &[FieldMeta],
    oids: &mut ParameterInference,
) {
    // A wildcard can expand to several slots; leave its output mapping to the binder.
    let positional = select.projection.len() == fields.len()
        && !select.projection.iter().any(|item| {
            matches!(item, ast::SelectItem::Wildcard)
                || matches!(item, ast::SelectItem::Column { name, .. } if name.ends_with('*'))
        });
    if positional {
        for (item, field) in select.projection.iter().zip(fields) {
            if let ast::SelectItem::Expr { expr, .. } = item {
                if field.data_type == DataType::Boolean {
                    infer_parameter_type_from_expected_expr(expr, &field.data_type, oids);
                }
            }
        }
    }
    if let Some(set) = &select.set {
        infer_insert_select_boolean_parameter_type_oids(&set.right, fields, oids);
    }
}

fn infer_select_parameter_type_oids(
    statement: &ast::SelectStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let field_types = source_field_type_map_with_ctes(&statement.source, &statement.ctes, catalog);
    for cte in &statement.ctes {
        match &cte.query {
            ast::CteQuery::Simple(statement) => {
                infer_parameter_type_oids_query(&statement.statement, catalog, oids);
            }
            ast::CteQuery::Recursive {
                base, recursive, ..
            } => {
                infer_parameter_type_oids_query(&base.statement, catalog, oids);
                infer_parameter_type_oids_query(&recursive.statement, catalog, oids);
            }
        }
    }
    infer_source_parameter_type_oids(&statement.source, &field_types, catalog, oids);
    for item in &statement.projection {
        infer_select_item_parameter_type_oids(item, &field_types, catalog, oids);
    }
    if let Some(filter) = &statement.filter {
        infer_parameter_type_from_expected_expr(filter, &DataType::Boolean, oids);
        infer_parameter_type_oids_expr(filter, &field_types, catalog, oids);
    }
    for expr in &statement.distinct_on {
        infer_parameter_type_oids_expr(expr, &field_types, catalog, oids);
    }
    for expr in &statement.group_by {
        infer_parameter_type_oids_expr(expr, &field_types, catalog, oids);
    }
    if let Some(having) = &statement.having {
        infer_parameter_type_from_expected_expr(having, &DataType::Boolean, oids);
        infer_parameter_type_oids_expr(having, &field_types, catalog, oids);
    }
    for order in &statement.order {
        infer_parameter_type_oids_expr(&order.expr, &field_types, catalog, oids);
    }
    for bound in statement.limit.iter().chain(&statement.offset) {
        pagination::infer_bound_parameter_types(bound, oids);
    }
    if let Some(set) = &statement.set {
        infer_select_parameter_type_oids(&set.right, catalog, oids);
    }
}

fn infer_source_parameter_type_oids(
    source: &ast::QuerySource,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    match source {
        ast::QuerySource::Aliased { source, .. } => {
            infer_source_parameter_type_oids(source, field_types, catalog, oids)
        }
        ast::QuerySource::Join {
            left, right, on, ..
        } => {
            infer_source_parameter_type_oids(left, field_types, catalog, oids);
            infer_source_parameter_type_oids(right, field_types, catalog, oids);
            infer_parameter_type_from_expected_expr(on, &DataType::Boolean, oids);
            infer_parameter_type_oids_expr(on, field_types, catalog, oids);
        }
        ast::QuerySource::Subquery { select, .. } => {
            infer_select_parameter_type_oids(select, catalog, oids);
        }
        ast::QuerySource::Collection(_)
        | ast::QuerySource::Cte(_)
        | ast::QuerySource::TableFunction { .. }
        | ast::QuerySource::SingleRow => {}
    }
}

fn infer_update_parameter_type_oids(
    statement: &ast::UpdateStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let Some(schema) = catalog.get_schema(&statement.table) else {
        return;
    };
    let field_types = field_type_map(schema.fields.iter());
    infer_assignment_parameter_type_oids(&statement.assignments, &field_types, catalog, oids);
    if let Some(filter) = &statement.filter {
        infer_parameter_type_from_expected_expr(filter, &DataType::Boolean, oids);
        infer_parameter_type_oids_expr(filter, &field_types, catalog, oids);
    }
    for item in &statement.returning {
        infer_select_item_parameter_type_oids(item, &field_types, catalog, oids);
    }
}

fn infer_delete_parameter_type_oids(
    statement: &ast::DeleteStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let Some(schema) = catalog.get_schema(&statement.table) else {
        return;
    };
    let field_types = field_type_map(schema.fields.iter());
    if let Some(filter) = &statement.filter {
        infer_parameter_type_from_expected_expr(filter, &DataType::Boolean, oids);
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
    oids: &mut ParameterInference,
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
    oids: &mut ParameterInference,
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
    oids: &mut ParameterInference,
) {
    match expr {
        ast::Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            if let Some(operand) = operand {
                infer_parameter_type_oids_expr(operand, field_types, catalog, oids);
            }
            for (when, then) in branches {
                if operand.is_none() {
                    infer_parameter_type_from_expected_expr(when, &DataType::Boolean, oids);
                }
                if let Some(data_type) = operand
                    .as_ref()
                    .and_then(|operand| column_expr_type(operand, field_types))
                {
                    oids.without_context_demands(|oids| {
                        infer_parameter_type_from_expected_expr(when, data_type, oids);
                    });
                }
                if let Some(operand) = operand {
                    record_operand_parameter_demand(when, operand, field_types, oids);
                }
                infer_parameter_type_oids_expr(when, field_types, catalog, oids);
                infer_parameter_type_oids_expr(then, field_types, catalog, oids);
            }
            if let Some(else_expr) = else_expr {
                infer_parameter_type_oids_expr(else_expr, field_types, catalog, oids);
            }
            infer_case_result_parameter_types(branches, else_expr.as_deref(), field_types, oids);
        }
        ast::Expr::Binary { left, op, right } => {
            infer_binary_parameter_type_oids(left, op, right, field_types, catalog, oids);
        }
        ast::Expr::InList { expr, values, .. } => {
            infer_in_list_parameter_type_oids(expr, values, field_types, catalog, oids);
        }
        ast::Expr::Between {
            expr, low, high, ..
        } => {
            if let Some(data_type) = column_expr_type(expr, field_types) {
                oids.without_context_demands(|oids| {
                    infer_parameter_type_from_expected_expr(low, data_type, oids);
                    infer_parameter_type_from_expected_expr(high, data_type, oids);
                });
            }
            record_operand_parameter_demand(low, expr, field_types, oids);
            record_operand_parameter_demand(high, expr, field_types, oids);
            record_operand_parameter_demand(expr, low, field_types, oids);
            record_operand_parameter_demand(expr, high, field_types, oids);
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
            infer_parameter_type_oids_expr(low, field_types, catalog, oids);
            infer_parameter_type_oids_expr(high, field_types, catalog, oids);
        }
        ast::Expr::IsNull { expr, .. } => {
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::Not { expr } => {
            infer_parameter_type_from_expected_expr(expr, &DataType::Boolean, oids);
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::Cast { expr, data_type } => {
            oids.without_context_demands(|oids| {
                infer_parameter_type_from_expected_expr(expr, data_type, oids);
            });
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
        | ast::Expr::IntegerLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => {}
    }
}

fn infer_in_list_parameter_type_oids(
    expr: &ast::Expr,
    values: &[ast::Expr],
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    if let Some(data_type) = column_expr_type(expr, field_types) {
        for value in values {
            oids.without_context_demands(|oids| {
                infer_parameter_type_from_expected_expr(value, data_type, oids);
            });
        }
    }
    infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
    for value in values {
        record_operand_parameter_demand(value, expr, field_types, oids);
        record_operand_parameter_demand(expr, value, field_types, oids);
        infer_parameter_type_oids_expr(value, field_types, catalog, oids);
    }
}

fn infer_binary_parameter_type_oids(
    left: &ast::Expr,
    op: &ast::BinaryOp,
    right: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    if matches!(op, ast::BinaryOp::And | ast::BinaryOp::Or) {
        infer_parameter_type_from_expected_expr(left, &DataType::Boolean, oids);
        infer_parameter_type_from_expected_expr(right, &DataType::Boolean, oids);
    } else {
        record_operand_parameter_demand(left, right, field_types, oids);
        record_operand_parameter_demand(right, left, field_types, oids);
    }
    // Preserve existing OID inference while recording the actual operand result type above.
    oids.without_context_demands(|oids| {
        if let Some(data_type) = column_expr_type(left, field_types) {
            infer_parameter_type_from_expected_expr(right, data_type, oids);
        }
        if let Some(data_type) = column_expr_type(right, field_types) {
            infer_parameter_type_from_expected_expr(left, data_type, oids);
        }
    });
    infer_parameter_type_oids_expr(left, field_types, catalog, oids);
    infer_parameter_type_oids_expr(right, field_types, catalog, oids);
}

fn record_operand_parameter_demand(
    parameter: &ast::Expr,
    operand: &ast::Expr,
    field_types: &FieldTypeMap,
    oids: &mut ParameterInference,
) {
    if let Some(data_type) = binder::known_expr_type(operand, field_types) {
        oids.record_expression_demand(parameter, &data_type);
    }
}

/// Types bare `$n` CASE results from the first sibling result whose type is
/// known, mirroring PostgreSQL's CASE result unification.
fn infer_case_result_parameter_types(
    branches: &[(ast::Expr, ast::Expr)],
    else_expr: Option<&ast::Expr>,
    field_types: &FieldTypeMap,
    oids: &mut ParameterInference,
) {
    let results = || branches.iter().map(|(_, then)| then).chain(else_expr);
    let Some(data_type) = results()
        .filter_map(|result| binder::known_expr_type(result, field_types))
        .find(|data_type| *data_type != DataType::Null)
    else {
        return;
    };
    for result in results() {
        infer_parameter_type_from_expected_expr(result, &data_type, oids);
    }
}

fn infer_function_parameter_type_oids(
    function: &ast::FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    for arg in &function.args {
        infer_parameter_type_oids_expr(arg, field_types, catalog, oids);
    }
}

fn infer_parameter_type_from_expected_expr(
    expr: &ast::Expr,
    data_type: &DataType,
    oids: &mut ParameterInference,
) {
    match expr {
        ast::Expr::Param(index) => set_parameter_type_oid(oids, *index, data_type),
        ast::Expr::Cast { expr, data_type } => {
            oids.without_context_demands(|oids| {
                infer_parameter_type_from_expected_expr(expr, data_type, oids);
            });
        }
        _ => {}
    }
}

fn set_parameter_type_oid(oids: &mut ParameterInference, index: usize, data_type: &DataType) {
    oids.record_type_demand(index, data_type);
    if let Some(oid) = oids.oids.get_mut(index) {
        if *oid == UNKNOWN_PARAMETER_TYPE_OID {
            *oid = i32::try_from(data_type.type_oid()).unwrap_or(i32::MAX);
        }
    }
}

pub(crate) fn source_field_type_map(
    source: &ast::QuerySource,
    catalog: &crate::catalog::Catalog,
) -> FieldTypeMap {
    source_field_type_map_with_ctes(source, &[], catalog)
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
        .map(|field| {
            (
                crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key(),
                field.data_type.clone(),
            )
        })
        .collect()
}

fn column_expr_type<'a>(expr: &ast::Expr, field_types: &'a FieldTypeMap) -> Option<&'a DataType> {
    match expr {
        ast::Expr::Column(column) => field_type_for_column(field_types, column),
        ast::Expr::Cast { expr, .. } => column_expr_type(expr, field_types),
        _ => None,
    }
}

pub(crate) fn field_type_for_column<'a>(
    field_types: &'a FieldTypeMap,
    column: &str,
) -> Option<&'a DataType> {
    let column = crate::sql::ColumnIdentifierPath::parse(column).ok()?;
    field_types
        .get(&column.namespace_key())
        .or_else(|| field_types.get(&column.lookup_key()))
        .or_else(|| field_types.get(&column.field_lookup_key()))
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
