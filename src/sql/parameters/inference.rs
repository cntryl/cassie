//! Statement and operand traversal with shared scalar function input inference.

use super::function_inputs::{
    infer_function_parameter_type_oids, infer_procedure_parameter_type_oids,
};
use super::{ParameterInference, UNKNOWN_PARAMETER_TYPE_OID};
use crate::catalog::FieldMeta;
use crate::sql::ast::Expr;
use crate::sql::{
    ast, binder, column_expr_type, field_type_for_column, field_type_map,
    source_field_type_map_with_ctes, FieldTypeMap, QueryStatement,
};
use crate::types::DataType;

#[path = "subquery_parameters.rs"]
mod subquery_parameters;

pub(super) fn infer_parameter_type_oids_query(
    statement: &QueryStatement,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    if !oids.check() {
        return;
    }
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
        QueryStatement::CallProcedure(statement) => {
            infer_procedure_parameter_type_oids(statement, catalog, oids);
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
    if !oids.check() {
        return;
    }
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
            ..
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
    if !oids.check() {
        return;
    }
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
    if !oids.check() {
        return;
    }
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
    if !oids.check() {
        return;
    }
    match source {
        ast::QuerySource::Values { rows } => {
            for expr in rows.iter().flatten() {
                infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
            }
        }

        ast::QuerySource::Aliased { source, .. } => {
            infer_source_parameter_type_oids(source, field_types, catalog, oids);
        }
        ast::QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            infer_source_parameter_type_oids(source, field_types, catalog, oids);
            infer_parameter_type_from_expected_expr(
                rate,
                &DataType::Extended(crate::types::extended::ExtendedType::Real),
                oids,
            );
            infer_parameter_type_oids_expr(rate, field_types, catalog, oids);
            if let Some(seed) = repeatable {
                infer_parameter_type_from_expected_expr(seed, &DataType::Float, oids);
                infer_parameter_type_oids_expr(seed, field_types, catalog, oids);
            }
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
    if !oids.check() {
        return;
    }
    let Some(schema) = catalog.get_schema(&statement.table) else {
        return;
    };
    let field_types = field_type_map(schema.fields.iter());
    for (index, (field, expression)) in statement.assignments.iter().enumerate() {
        let expected = field_type_for_column(&field_types, field).and_then(|field_type| {
            statement
                .array_targets
                .iter()
                .find(|target| target.assignment_index == index)
                .map_or(Some(field_type), |target| {
                    let DataType::Array(element_type) = field_type else {
                        return None;
                    };
                    if target
                        .subscripts
                        .iter()
                        .any(|subscript| matches!(subscript, ast::ArraySubscript::Slice { .. }))
                    {
                        Some(field_type)
                    } else {
                        Some(element_type.as_ref())
                    }
                })
        });
        if let Some(expected) = expected {
            infer_parameter_type_from_expected_expr(expression, expected, oids);
        }
        infer_parameter_type_oids_expr(expression, &field_types, catalog, oids);
        if let Some(target) = statement
            .array_targets
            .iter()
            .find(|target| target.assignment_index == index)
        {
            for subscript in &target.subscripts {
                match subscript {
                    ast::ArraySubscript::Index(index) => {
                        infer_parameter_type_from_expected_expr(index, &DataType::Int, oids);
                        infer_parameter_type_oids_expr(index, &field_types, catalog, oids);
                    }
                    ast::ArraySubscript::Slice { lower, upper } => {
                        for bound in lower.iter().chain(upper) {
                            infer_parameter_type_from_expected_expr(bound, &DataType::Int, oids);
                            infer_parameter_type_oids_expr(bound, &field_types, catalog, oids);
                        }
                    }
                }
            }
        }
    }
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
    if !oids.check() {
        return;
    }
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
    if !oids.check() {
        return;
    }
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
    if !oids.check() {
        return;
    }
    match item {
        ast::SelectItem::Wildcard | ast::SelectItem::Column { .. } => {}
        ast::SelectItem::Function { function, .. } => {
            infer_function_parameter_type_oids(function, field_types, catalog, oids);
        }
        ast::SelectItem::Expr { expr, .. } => {
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::SelectItem::WindowFunction { function, .. } => {
            let metadata = crate::sql::functions::window_function(&function.name);
            let integer_argument = if function.name.eq_ignore_ascii_case("ntile") {
                function.args.first()
            } else {
                metadata.and_then(|metadata| {
                    let argument_index =
                        metadata
                            .signature(function.args.len())?
                            .iter()
                            .position(|argument| {
                                *argument == crate::sql::functions::WindowArgumentType::Int4
                            })?;
                    function.args.get(argument_index)
                })
            };
            if let Some(argument) = integer_argument {
                infer_parameter_type_from_expected_expr(argument, &DataType::Int, oids);
            }
            let default_input = if matches!(
                metadata.map(|metadata| metadata.id),
                Some(
                    crate::sql::functions::FunctionId::Lag
                        | crate::sql::functions::FunctionId::Lead
                )
            ) {
                function
                    .args
                    .get(2)
                    .zip(function.args.first())
                    .and_then(|(default, value)| {
                        let value_type = known_expr_type_with_parameters(value, field_types, oids);
                        let default_type =
                            known_expr_type_with_parameters(default, field_types, oids);
                        let (input, resolve_unknown) = match (value_type, default_type) {
                            (Some(data_type), _) if data_type != DataType::Null => {
                                (Some((default, data_type)), false)
                            }
                            (_, Some(data_type)) if data_type != DataType::Null => {
                                (Some((value, data_type)), false)
                            }
                            _ => (None, true),
                        };
                        if resolve_unknown {
                            infer_parameter_type_from_expected_expr(value, &DataType::Text, oids);
                            infer_parameter_type_from_expected_expr(default, &DataType::Text, oids);
                        }
                        input
                    })
            } else {
                None
            };
            if let Some((default, data_type)) = default_input {
                infer_parameter_type_from_expected_expr(default, &data_type, oids);
            }
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

fn known_expr_type_with_parameters(
    expr: &ast::Expr,
    field_types: &FieldTypeMap,
    oids: &ParameterInference,
) -> Option<DataType> {
    binder::known_expr_type(expr, field_types).or_else(|| {
        let ast::Expr::Param(index) = expr else {
            return None;
        };
        let oid = *oids.oids.get(*index)?;
        if matches!(oid, 0 | UNKNOWN_PARAMETER_TYPE_OID) {
            return None;
        }
        binder::parameter_data_type_for_oid(oid)
    })
}

pub(super) fn infer_parameter_type_oids_expr(
    expr: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    if !oids.check() {
        return;
    }
    match expr {
        ast::Expr::Case {
            operand,
            branches,
            else_expr,
        } => infer_case_expression_parameter_types(
            operand.as_deref(),
            branches,
            else_expr.as_deref(),
            field_types,
            catalog,
            oids,
        ),
        ast::Expr::RowCompare(comparison) => {
            for (left, right) in comparison.left.iter().zip(&comparison.right) {
                infer_binary_parameter_type_oids(
                    left,
                    &comparison.op.binary(),
                    right,
                    field_types,
                    catalog,
                    oids,
                );
            }
        }
        ast::Expr::ArraySubscript { array, subscripts } => {
            for subscript in subscripts {
                match subscript {
                    ast::ArraySubscript::Index(index) => {
                        infer_parameter_type_from_expected_expr(index, &DataType::Int, oids);
                        infer_parameter_type_oids_expr(index, field_types, catalog, oids);
                    }
                    ast::ArraySubscript::Slice { lower, upper } => {
                        for bound in lower.iter().chain(upper) {
                            infer_parameter_type_from_expected_expr(bound, &DataType::Int, oids);
                            infer_parameter_type_oids_expr(bound, field_types, catalog, oids);
                        }
                    }
                }
            }
            infer_parameter_type_oids_expr(array, field_types, catalog, oids);
        }
        ast::Expr::Binary { left, op, right } => {
            infer_binary_parameter_type_oids(left, op, right, field_types, catalog, oids);
        }
        ast::Expr::InList { expr, values, .. } => {
            infer_in_list_parameter_type_oids(expr, values, field_types, catalog, oids);
        }
        expression @ ast::Expr::Like { .. } => {
            infer_like_parameter_type_oids(expression, field_types, catalog, oids);
        }
        ast::Expr::Between {
            expr, low, high, ..
        } => {
            infer_between_parameter_type_oids([expr, low, high], field_types, catalog, oids);
        }
        ast::Expr::TruthTest { expr, .. } | ast::Expr::Not { expr } => {
            infer_parameter_type_from_expected_expr(expr, &DataType::Boolean, oids);
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::UnaryNumeric {
            op: ast::UnaryNumericOp::SquareRoot | ast::UnaryNumericOp::CubeRoot,
            expr,
        } => {
            infer_parameter_type_from_expected_expr(expr, &DataType::Float, oids);
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::IsNull { expr, .. }
        | ast::Expr::UnaryNumeric { expr, .. }
        | ast::Expr::IntegerNegate { expr, .. }
        | ast::Expr::Collate { expr, .. } => {
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::Cast {
            expr, data_type, ..
        } => {
            oids.without_context_demands(|oids| {
                infer_parameter_type_from_expected_expr(expr, data_type, oids);
            });
            infer_parameter_type_oids_expr(expr, field_types, catalog, oids);
        }
        ast::Expr::Exists(_) | ast::Expr::Subquery(_) => {
            subquery_parameters::infer(expr, field_types, catalog, oids);
        }
        ast::Expr::Function(function) => {
            infer_function_parameter_type_oids(function, field_types, catalog, oids);
        }
        ast::Expr::Column(_)
        | ast::Expr::Param(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::IntegerLiteral(_)
        | ast::Expr::NumericLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => {}
    }
}

fn infer_case_expression_parameter_types(
    operand: Option<&ast::Expr>,
    branches: &[(ast::Expr, ast::Expr)],
    else_expr: Option<&ast::Expr>,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    if let Some(operand) = operand {
        infer_parameter_type_oids_expr(operand, field_types, catalog, oids);
    }
    for (when, then) in branches {
        if operand.is_none() {
            infer_parameter_type_from_expected_expr(when, &DataType::Boolean, oids);
        }
        if let Some(data_type) = operand.and_then(|operand| column_expr_type(operand, field_types))
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
    infer_case_result_parameter_types(branches, else_expr, field_types, oids);
}

fn infer_between_parameter_type_oids(
    [expr, low, high]: [&ast::Expr; 3],
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
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

fn infer_in_list_parameter_type_oids(
    expr: &ast::Expr,
    values: &[ast::Expr],
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    if !oids.check() {
        return;
    }
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

fn infer_json_extraction(
    left: &ast::Expr,
    right: &ast::Expr,
    path: bool,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let selector_type = if path {
        DataType::Array(Box::new(DataType::Text))
    } else if matches!(
        binder::known_expr_type(right, field_types),
        Some(DataType::Int | DataType::SmallInt | DataType::BigInt)
    ) {
        DataType::Int
    } else {
        DataType::Text
    };
    infer_parameter_type_from_expected_expr(
        left,
        &binder::known_expr_type(left, field_types)
            .filter(DataType::is_json)
            .unwrap_or(DataType::Jsonb),
        oids,
    );
    infer_parameter_type_from_expected_expr(right, &selector_type, oids);
    infer_parameter_type_oids_expr(left, field_types, catalog, oids);
    infer_parameter_type_oids_expr(right, field_types, catalog, oids);
}

fn infer_binary_parameter_type_oids(
    left: &ast::Expr,
    op: &ast::BinaryOp,
    right: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    if !oids.check() {
        return;
    }
    if matches!(op, ast::BinaryOp::Sub)
        && binder::known_expr_type(left, field_types).is_some_and(|kind| kind.is_json())
    {
        infer_parameter_type_from_expected_expr(right, &DataType::Text, oids);
        infer_parameter_type_oids_expr(left, field_types, catalog, oids);
        infer_parameter_type_oids_expr(right, field_types, catalog, oids);
        return;
    }
    if let ast::BinaryOp::JsonExtract { path, .. } = op {
        infer_json_extraction(left, right, *path, field_types, catalog, oids);
        return;
    }
    if matches!(
        op,
        ast::BinaryOp::JsonPathExists | ast::BinaryOp::JsonPathMatch
    ) {
        infer_jsonpath_operator(left, right, field_types, catalog, oids);
        return;
    }
    if matches!(op, ast::BinaryOp::JsonbDeletePath) {
        infer_json_delete_path(left, right, field_types, catalog, oids);
        return;
    }
    match op {
        ast::BinaryOp::JsonbKeyExists
        | ast::BinaryOp::JsonbKeyExistsAny
        | ast::BinaryOp::JsonbKeyExistsAll => {
            infer_json_key_operator(left, op, right, field_types, catalog, oids);
            return;
        }
        _ => {}
    }
    if matches!(op, ast::BinaryOp::Bitwise { operation } | ast::BinaryOp::Integer { operation, .. } if operation.is_shift())
    {
        if let ast::Expr::Param(slot) = left {
            if oids
                .oids
                .get(*slot)
                .is_some_and(|oid| matches!(*oid, 0 | 705))
            {
                infer_parameter_type_from_expected_expr(left, &DataType::Int, oids);
            }
        }
        infer_parameter_type_from_expected_expr(right, &DataType::Int, oids);
        infer_parameter_type_oids_expr(left, field_types, catalog, oids);
        infer_parameter_type_oids_expr(right, field_types, catalog, oids);
        return;
    }
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

fn infer_jsonpath_operator(
    left: &ast::Expr,
    right: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let document_type = binder::known_expr_type(left, field_types)
        .filter(DataType::is_json)
        .unwrap_or(DataType::Jsonb);
    infer_parameter_type_from_expected_expr(left, &document_type, oids);
    infer_parameter_type_from_expected_expr(right, &DataType::JsonPath, oids);
    infer_parameter_type_oids_expr(left, field_types, catalog, oids);
    infer_parameter_type_oids_expr(right, field_types, catalog, oids);
}

fn infer_json_delete_path(
    left: &ast::Expr,
    right: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let document_type = binder::known_expr_type(left, field_types)
        .filter(DataType::is_json)
        .unwrap_or(DataType::Jsonb);
    let path_type = DataType::Array(Box::new(DataType::Text));
    infer_parameter_type_from_expected_expr(left, &document_type, oids);
    infer_parameter_type_from_expected_expr(right, &path_type, oids);
    infer_parameter_type_oids_expr(left, field_types, catalog, oids);
    infer_parameter_type_oids_expr(right, field_types, catalog, oids);
}

fn infer_json_key_operator(
    left: &ast::Expr,
    op: &ast::BinaryOp,
    right: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let document_type = binder::known_expr_type(left, field_types)
        .filter(DataType::is_json)
        .unwrap_or(DataType::Jsonb);
    let key_type = if matches!(op, ast::BinaryOp::JsonbKeyExists) {
        DataType::Text
    } else {
        DataType::Array(Box::new(DataType::Text))
    };
    infer_parameter_type_from_expected_expr(left, &document_type, oids);
    infer_parameter_type_from_expected_expr(right, &key_type, oids);
    infer_parameter_type_oids_expr(left, field_types, catalog, oids);
    infer_parameter_type_oids_expr(right, field_types, catalog, oids);
}

fn record_operand_parameter_demand(
    parameter: &ast::Expr,
    operand: &ast::Expr,
    field_types: &FieldTypeMap,
    oids: &mut ParameterInference,
) {
    if !oids.check() {
        return;
    }
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
    if !oids.check() {
        return;
    }
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

pub(super) fn infer_parameter_type_from_expected_expr(
    expr: &ast::Expr,
    data_type: &DataType,
    oids: &mut ParameterInference,
) {
    if !oids.check() {
        return;
    }
    match expr {
        ast::Expr::Param(index) => set_parameter_type_oid(oids, *index, data_type),
        ast::Expr::ArraySubscript { array, subscripts } => {
            let array_type = match data_type {
                DataType::Array(_) => data_type.clone(),
                data_type => DataType::Array(Box::new(data_type.clone())),
            };
            infer_parameter_type_from_expected_expr(array, &array_type, oids);
            for subscript in subscripts {
                match subscript {
                    ast::ArraySubscript::Index(index) => {
                        infer_parameter_type_from_expected_expr(index, &DataType::Int, oids);
                    }
                    ast::ArraySubscript::Slice { lower, upper } => {
                        for bound in lower.iter().chain(upper) {
                            infer_parameter_type_from_expected_expr(bound, &DataType::Int, oids);
                        }
                    }
                }
            }
        }
        ast::Expr::Cast {
            expr, data_type, ..
        } => {
            oids.without_context_demands(|oids| {
                infer_parameter_type_from_expected_expr(expr, data_type, oids);
            });
        }
        ast::Expr::Collate { expr, .. } => {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
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
        Expr::Cast {
            expr, data_type, ..
        } => {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
            infer_cast_parameters(expr, oids);
        }
        Expr::Collate { expr, .. } => infer_bound_parameter_types(expr, oids),
        _ => infer_cast_parameters(expr, oids),
    }
}

fn infer_cast_parameters(expr: &Expr, oids: &mut ParameterInference) {
    match expr {
        Expr::Cast {
            expr, data_type, ..
        } => {
            infer_parameter_type_from_expected_expr(expr, data_type, oids);
            infer_cast_parameters(expr, oids);
        }
        Expr::UnaryNumeric { expr, .. } | Expr::IntegerNegate { expr, .. } => {
            infer_cast_parameters(expr, oids);
        }
        Expr::Collate { expr, .. } => infer_cast_parameters(expr, oids),
        Expr::Binary { left, right, .. } => {
            infer_cast_parameters(left, oids);
            infer_cast_parameters(right, oids);
        }
        _ => {}
    }
}

fn infer_like_parameter_type_oids(
    expression: &ast::Expr,
    field_types: &FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let ast::Expr::Like {
        expr,
        pattern,
        escape,
        ..
    } = expression
    else {
        unreachable!("LIKE inference dispatch")
    };
    for operand in [expr, pattern, escape] {
        infer_parameter_type_from_expected_expr(operand, &DataType::Text, oids);
        infer_parameter_type_oids_expr(operand, field_types, catalog, oids);
    }
}
