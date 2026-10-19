//! Narrow `numeric` decimal literals compared with `double precision`.
//!
//! PostgreSQL resolves `float8_expr op 1.5` to the `double precision`
//! operator and folds the implicit `numeric` to `float8` cast of the constant.
//! Folding it here keeps float index probes and float comparisons native.
use super::super::coalesce_results::ResultTypes;
use crate::sql::ast::Expr;
use crate::types::DataType;

fn narrow(literal: &mut Expr) {
    let Expr::NumericLiteral(text) = literal else {
        return;
    };
    if let Ok(value) = crate::types::float_input::parse(text) {
        *literal = Expr::NumberLiteral(value);
    }
}

/// Folds a decimal literal operand when its peer is `double precision`.
pub(super) fn narrow_pair(left: &mut Expr, right: &mut Expr, types: &ResultTypes) {
    if matches!(right, Expr::NumericLiteral(_))
        && types.expression_type(left) == Some(DataType::Float)
    {
        narrow(right);
    } else if matches!(left, Expr::NumericLiteral(_))
        && types.expression_type(right) == Some(DataType::Float)
    {
        narrow(left);
    }
}
