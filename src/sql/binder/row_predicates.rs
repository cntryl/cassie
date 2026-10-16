//! Qualify each row field independently before runtime short-circuiting.
use super::super::coalesce_results::ResultTypes;
use crate::app::CassieError;
use crate::sql::ast::{Expr, RowComparison};
use crate::types::DataType;

pub(super) fn qualify(
    comparison: &mut RowComparison,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if comparison.left.len() != comparison.right.len() {
        return Err(CassieError::InvalidQuery(
            "unequal number of entries in row expressions".into(),
        ));
    }
    if comparison.left.is_empty() && !comparison.op.null_safe() {
        return Err(CassieError::Unsupported(
            "cannot compare rows of zero length".into(),
        ));
    }
    if comparison.left.len() > 1664 {
        return Err(CassieError::ResourceLimit(
            "ROW expressions can have at most 1664 entries".into(),
        ));
    }
    for (left, right) in comparison.left.iter_mut().zip(&mut comparison.right) {
        numeric_literals(left)?;
        numeric_literals(right)?;
        resolve_operators(left, types)?;
        resolve_operators(right, types)?;
        let left_type = known_type(left, types);
        let right_type = known_type(right, types);
        let selected = left_type.as_ref().or(right_type.as_ref());
        if let Some(selected) = selected {
            if !primitive(selected) {
                return Err(CassieError::Unsupported(
                    "row comparison for this field type is not yet supported".into(),
                ));
            }
            coerce_unknown(left, selected);
            coerce_unknown(right, selected);
        }
        if let (Some(left_type), Some(right_type)) = (&left_type, &right_type) {
            if !primitive(left_type) || !primitive(right_type) {
                return Err(CassieError::Unsupported(
                    "row comparison for this field type is not yet supported".into(),
                ));
            }
            if numeric(left_type) && numeric(right_type) {
                if *left_type == DataType::Float || *right_type == DataType::Float {
                    cast(left, &DataType::Float);
                    cast(right, &DataType::Float);
                }
            } else if left_type != right_type && !(text(left_type) && text(right_type)) {
                return Err(CassieError::Execution(format!(
                    "operator does not exist: {} {:?} {}",
                    left_type.type_name(),
                    comparison.op,
                    right_type.type_name()
                )));
            }
        }
    }
    Ok(())
}

fn known_type(expr: &Expr, types: &ResultTypes) -> Option<DataType> {
    if matches!(expr, Expr::StringLiteral(_) | Expr::Null) {
        return None;
    }
    types
        .expression_type(expr)
        .filter(|kind| *kind != DataType::Null)
}
fn primitive(kind: &DataType) -> bool {
    numeric(kind) || text(kind) || *kind == DataType::Boolean
}
fn numeric(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
    )
}
fn text(kind: &DataType) -> bool {
    matches!(kind, DataType::Text | DataType::Varchar { .. })
}
fn coerce_unknown(expr: &mut Expr, kind: &DataType) {
    if matches!(expr, Expr::StringLiteral(_) | Expr::Null) {
        let kind = if matches!(kind, DataType::Varchar { .. }) {
            &DataType::Varchar { length: None }
        } else {
            kind
        };
        cast(expr, kind);
    }
}
fn cast(expr: &mut Expr, kind: &DataType) {
    if matches!(expr, Expr::Cast { data_type, .. } if data_type == kind) {
        return;
    }
    let original = std::mem::replace(expr, Expr::Null);
    *expr = Expr::Cast {
        expr: Box::new(original),
        data_type: kind.clone(),
    };
}

fn numeric_literals(expression: &Expr) -> Result<(), CassieError> {
    match expression {
        Expr::Cast {
            expr,
            data_type: DataType::Float,
        } if matches!(expr.as_ref(), Expr::NumberLiteral(_)) => Ok(()),
        Expr::NumberLiteral(_) => Err(CassieError::Unsupported(
            "NUMERIC row comparison operands are not yet supported".into(),
        )),
        _ => expression.try_visit_children(numeric_literals),
    }
}

fn resolve_operators(expression: &mut Expr, types: &ResultTypes) -> Result<(), CassieError> {
    expression.try_visit_children_mut(|child| resolve_operators(child, types))?;
    if let Expr::Binary { left, op, right } = expression {
        let left = types.expression_type(left);
        let right = types.expression_type(right);
        if !super::super::integer_operators::resolve(op, left.as_ref(), right.as_ref())?
            && left.is_some()
            && right.is_some()
        {
            let floating = (left == Some(DataType::Float) || right == Some(DataType::Float))
                && [left.as_ref(), right.as_ref()]
                    .into_iter()
                    .flatten()
                    .all(|kind| numeric(kind) || *kind == DataType::Null);
            if !floating {
                return Err(CassieError::Unsupported(
                    "row arithmetic requires resolved numeric operands".into(),
                ));
            }
        }
    }
    Ok(())
}
