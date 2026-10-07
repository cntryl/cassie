//! One operation/type decision shared by planning eligibility and runtime dispatch.
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::{DataType, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Capability {
    NativeTyped,
    BoundedScalarExpression,
    ExistingUnsupported,
}

/// The finite typed hash join key domain uses shared numeric equality.
pub(crate) fn join_keys(left: &DataType, right: &DataType) -> Capability {
    let numeric = |data_type: &DataType| {
        matches!(
            data_type,
            DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Null
        )
    };
    if (numeric(left) && numeric(right))
        || (matches!(left, DataType::Boolean | DataType::Null)
            && matches!(right, DataType::Boolean | DataType::Null))
    {
        Capability::NativeTyped
    } else {
        Capability::BoundedScalarExpression
    }
}

pub(crate) fn expression(
    expr: &Expr,
    schema: &[(String, DataType)],
    params: &[Value],
) -> Capability {
    match expr {
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::Null => Capability::NativeTyped,
        Expr::IsNull { expr, .. } | Expr::Not { expr } => expression(expr, schema, params),
        Expr::Binary { left, op, right } => {
            if expression(left, schema, params) != Capability::NativeTyped
                || expression(right, schema, params) != Capability::NativeTyped
            {
                return Capability::BoundedScalarExpression;
            }
            match op {
                BinaryOp::And | BinaryOp::Or
                    if boolean_type(left, schema, params)
                        && boolean_type(right, schema, params) =>
                {
                    Capability::NativeTyped
                }
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte
                    if numeric_type(left, schema, params)
                        && numeric_type(right, schema, params) =>
                {
                    Capability::NativeTyped
                }
                _ => Capability::BoundedScalarExpression,
            }
        }
        Expr::Exists(_) => Capability::ExistingUnsupported,
        _ => Capability::BoundedScalarExpression,
    }
}

fn operand_type(expr: &Expr, schema: &[(String, DataType)], params: &[Value]) -> Option<DataType> {
    match expr {
        Expr::Column(name) => schema
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, data_type)| data_type.clone()),
        Expr::IntegerLiteral(_) => Some(DataType::BigInt),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::Null => Some(DataType::Null),
        Expr::Param(index) => match params.get(*index) {
            Some(Value::Int64(_)) => Some(DataType::BigInt),
            Some(Value::Float64(_)) => Some(DataType::Float),
            Some(Value::Bool(_)) => Some(DataType::Boolean),
            Some(Value::Null) | None => Some(DataType::Null),
            _ => None,
        },
        Expr::BoolLiteral(_)
        | Expr::Not { .. }
        | Expr::IsNull { .. }
        | Expr::Binary {
            op:
                BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte,
            ..
        } => Some(DataType::Boolean),
        _ => None,
    }
}

fn numeric_type(expr: &Expr, schema: &[(String, DataType)], params: &[Value]) -> bool {
    matches!(
        operand_type(expr, schema, params),
        Some(
            DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Null
        )
    )
}

fn boolean_type(expr: &Expr, schema: &[(String, DataType)], params: &[Value]) -> bool {
    matches!(
        operand_type(expr, schema, params),
        Some(DataType::Boolean | DataType::Null)
    )
}
