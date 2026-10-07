//! The selected integer-only LIMIT/OFFSET contract.

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{BinaryOp, Expr, ParsedStatement};
use crate::types::{DataType, Value};

mod admission;
mod traversal;

fn bound_needs_resolution(bound: Option<&Expr>) -> bool {
    bound.is_some_and(|expr| !matches!(expr, Expr::IntegerLiteral(value) if *value >= 0))
}

pub(crate) fn plan_needs_resolution(
    plan: &crate::planner::logical::LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    traversal::plan_needs_resolution(plan, controls)
}

pub(crate) fn resolve_plan(
    plan: &crate::planner::logical::LogicalPlan,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<
    (
        crate::planner::logical::LogicalPlan,
        crate::runtime::QueryMemoryReservation,
    ),
    CassieError,
> {
    use crate::sql::ast::{QueryStatement, SelectStatement};
    for bound in plan.limit.iter().chain(&plan.offset) {
        if !is_admitted(bound) {
            return Err(type_error());
        }
    }
    let reservation = admission::reserve_plan_clone(plan, controls)?;
    let mut statement = ParsedStatement {
        raw_sql: String::new(),
        statement: QueryStatement::Select(SelectStatement {
            source: plan.source.clone(),
            ctes: plan.ctes.clone(),
            recursive: false,
            distinct: plan.distinct,
            distinct_on: plan.distinct_on.clone(),
            projection: plan.projection.clone(),
            filter: plan.filter.clone(),
            group_by: plan.group_by.clone(),
            having: plan.having.clone(),
            order: plan.order.clone(),
            limit: plan.limit.clone(),
            offset: plan.offset.clone(),
            set: plan.set.clone(),
        }),
    };
    // Direct executor values carry concrete integer domains. The application
    // validates the original prepared OIDs before reaching this fallback.
    let oids: Vec<_> = params
        .iter()
        .map(|value| {
            if matches!(value, Value::Int64(_)) {
                20
            } else {
                705
            }
        })
        .collect();
    resolve_statement(&mut statement, params, &oids, controls)?;
    let QueryStatement::Select(select) = statement.statement else {
        unreachable!()
    };
    Ok((
        crate::planner::logical::LogicalPlan {
            command: plan.command.clone(),
            collection: plan.collection.clone(),
            source: select.source,
            ctes: select.ctes,
            distinct: select.distinct,
            distinct_on: select.distinct_on,
            projection: select.projection,
            filter: select.filter,
            group_by: select.group_by,
            having: select.having,
            order: select.order,
            limit: select.limit,
            offset: select.offset,
            set: select.set,
        },
        reservation,
    ))
}

pub(super) fn infer_bound_parameter_types(expr: &Expr, oids: &mut super::ParameterInference) {
    match expr {
        Expr::Param(index) => super::set_parameter_type_oid(oids, *index, &DataType::BigInt),
        Expr::Cast { expr, data_type } => {
            super::infer_parameter_type_from_expected_expr(expr, data_type, oids);
            infer_cast_parameters(expr, oids);
        }
        _ => infer_cast_parameters(expr, oids),
    }
}

fn infer_cast_parameters(expr: &Expr, oids: &mut super::ParameterInference) {
    match expr {
        Expr::Cast { expr, data_type } => {
            super::infer_parameter_type_from_expected_expr(expr, data_type, oids);
            infer_cast_parameters(expr, oids);
        }
        Expr::Binary { left, right, .. } => {
            infer_cast_parameters(left, oids);
            infer_cast_parameters(right, oids);
        }
        _ => {}
    }
}

pub(crate) fn is_admitted(expr: &Expr) -> bool {
    is_admitted_at_depth(expr, 0)
}

fn is_admitted_at_depth(expr: &Expr, depth: usize) -> bool {
    // Match the existing SQL nesting envelope for direct constructed bounds.
    if depth > 128 {
        return false;
    }
    match expr {
        Expr::IntegerLiteral(_) | Expr::Null | Expr::Param(_) => true,
        Expr::Cast { expr, data_type } => {
            is_integer(data_type) && is_admitted_at_depth(expr, depth + 1)
        }
        Expr::Binary { left, op, right } => {
            matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
                && is_admitted_at_depth(left, depth + 1)
                && is_admitted_at_depth(right, depth + 1)
        }
        _ => false,
    }
}

fn is_integer(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::SmallInt | DataType::Int | DataType::BigInt
    )
}

pub(crate) fn has_parameters(expr: &Expr) -> bool {
    match expr {
        Expr::Param(_) => true,
        Expr::Cast { expr, .. } => has_parameters(expr),
        Expr::Binary { left, right, .. } => has_parameters(left) || has_parameters(right),
        _ => false,
    }
}

/// A conservative planning bound. Runtime execution normalizes parameter bounds
/// first; Describe does not evaluate them or open a source.
pub(crate) fn constant_bound(bound: Option<&Expr>) -> Option<i64> {
    match bound? {
        Expr::IntegerLiteral(value) if *value >= 0 => Some(*value),
        _ => None,
    }
}

pub(crate) fn validate_statement(
    statement: &mut ParsedStatement,
    parameter_oids: Option<&[i32]>,
) -> Result<(), CassieError> {
    traversal::visit_statement(statement, &mut |bound, _| {
        if let Some(expr) = bound {
            if !is_admitted(expr) {
                return Err(type_error());
            }
            if let Some(oids) = parameter_oids {
                validate_parameters(expr, oids, false, None)?;
            }
        }
        Ok(())
    })
}

fn validate_parameters(
    expr: &Expr,
    oids: &[i32],
    arithmetic: bool,
    cast: Option<&DataType>,
) -> Result<(), CassieError> {
    match expr {
        Expr::Param(index) => {
            let oid = oids.get(*index).copied().unwrap_or(705);
            if !matches!(oid, 0 | 705 | 20 | 21 | 23) {
                return Err(type_error());
            }
            if arithmetic && cast.is_none() && matches!(oid, 0 | 705) {
                return Err(CassieError::Execution(
                    "pagination operator is ambiguous for unknown parameters".into(),
                ));
            }
        }
        Expr::Cast { expr, data_type } => {
            validate_parameters(expr, oids, arithmetic, Some(data_type))?;
        }
        Expr::Binary { left, right, .. } => {
            // An outer cast does not select the argument operators.
            validate_parameters(left, oids, true, None)?;
            validate_parameters(right, oids, true, None)?;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn resolve_statement(
    statement: &mut ParsedStatement,
    params: &[Value],
    declared_oids: &[i32],
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    validate_statement(statement, Some(declared_oids))?;
    let mut parameter_dependent = false;
    traversal::visit_statement(statement, &mut |bound, offset| {
        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        if let Some(expr) = bound.as_ref() {
            parameter_dependent |= has_parameters(expr);
            let value = evaluate(expr, params, declared_oids, controls)?.0;
            if value.is_some_and(|value| value < 0) {
                return Err(CassieError::Execution(if offset {
                    "OFFSET must not be negative".into()
                } else {
                    "LIMIT must not be negative".into()
                }));
            }
            *bound = value.map(Expr::IntegerLiteral);
        }
        Ok(())
    })?;
    Ok(parameter_dependent)
}

fn type_error() -> CassieError {
    CassieError::Execution("pagination requires an admitted integer expression".into())
}

fn overflow() -> CassieError {
    CassieError::Execution(crate::executor::filter::BIGINT_OUT_OF_RANGE.into())
}

fn checked_domain(value: i64, domain: &DataType) -> Result<i64, CassieError> {
    let valid = match domain {
        DataType::SmallInt => i16::try_from(value).is_ok(),
        DataType::Int => i32::try_from(value).is_ok(),
        DataType::BigInt => true,
        _ => false,
    };
    if valid {
        Ok(value)
    } else {
        Err(overflow())
    }
}

fn evaluate(
    expr: &Expr,
    params: &[Value],
    declared_oids: &[i32],
    controls: &QueryExecutionControls,
) -> Result<(Option<i64>, DataType), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    match expr {
        Expr::Null => Ok((None, DataType::Int)),
        Expr::IntegerLiteral(value) => Ok((
            Some(*value),
            if i32::try_from(*value).is_ok() {
                DataType::Int
            } else {
                DataType::BigInt
            },
        )),
        Expr::Param(index) => {
            let domain = match declared_oids.get(*index) {
                Some(21) => DataType::SmallInt,
                Some(23) => DataType::Int,
                _ => DataType::BigInt,
            };
            match params.get(*index) {
                Some(Value::Int64(value)) => Ok((Some(checked_domain(*value, &domain)?), domain)),
                Some(Value::Null) => Ok((None, domain)),
                _ => Err(type_error()),
            }
        }
        Expr::Cast { expr, data_type } if is_integer(data_type) => {
            let value = evaluate(expr, params, declared_oids, controls)?.0;
            Ok((
                value
                    .map(|value| checked_domain(value, data_type))
                    .transpose()?,
                data_type.clone(),
            ))
        }
        Expr::Binary { left, op, right } => {
            let (left, left_type) = evaluate(left, params, declared_oids, controls)?;
            let (right, right_type) = evaluate(right, params, declared_oids, controls)?;
            let domain = if left_type == DataType::BigInt || right_type == DataType::BigInt {
                DataType::BigInt
            } else if left_type == DataType::Int || right_type == DataType::Int {
                DataType::Int
            } else {
                DataType::SmallInt
            };
            let Some((left, right)) = left.zip(right) else {
                return Ok((None, domain));
            };
            let value = match op {
                BinaryOp::Add => left.checked_add(right),
                BinaryOp::Sub => left.checked_sub(right),
                BinaryOp::Mul => left.checked_mul(right),
                _ => return Err(type_error()),
            }
            .ok_or_else(overflow)?;
            Ok((Some(checked_domain(value, &domain)?), domain))
        }
        _ => Err(type_error()),
    }
}
