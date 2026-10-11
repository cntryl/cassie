//! Current scalar parameter inference, extracted without semantic changes.

use super::{ParameterInference, UNKNOWN_PARAMETER_TYPE_OID};
use crate::catalog::FieldMeta;
use crate::sql::ast::Expr;
use crate::sql::{
    ast, binder, column_expr_type, field_type_for_column, field_type_map,
    source_field_type_map_with_ctes, FieldTypeMap, QueryStatement,
};
use crate::types::DataType;

pub(super) fn infer_parameter_type_oids_query(
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
        infer_bound_parameter_types(bound, oids);
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
            infer_source_parameter_type_oids(source, field_types, catalog, oids);
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

fn infer_bound_parameter_types(expr: &Expr, oids: &mut ParameterInference) {
    match expr {
        Expr::Param(index) => set_parameter_type_oid(oids, *index, &DataType::BigInt),
        Expr::Cast { expr, data_type } => {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
            infer_cast_parameters(expr, oids);
        }
        _ => infer_cast_parameters(expr, oids),
    }
}

fn infer_cast_parameters(expr: &Expr, oids: &mut ParameterInference) {
    match expr {
        Expr::Cast { expr, data_type } => {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
            infer_cast_parameters(expr, oids);
        }
        Expr::Binary { left, right, .. } => {
            infer_cast_parameters(left, oids);
            infer_cast_parameters(right, oids);
        }
        _ => {}
    }
}
