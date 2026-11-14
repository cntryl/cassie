//! Named enum columns of a set operation must be one enum type (#827).
//!
//! PostgreSQL resolves each output column of `UNION`, `INTERSECT` and `EXCEPT`
//! to a common type. Two different enum types have none (`42846`); without
//! this check a branch's values would be relabelled as the other type.

use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::{SelectStatement, SetOperator};
use crate::types::{DataType, EnumRef};

use super::super::{BindingContext, Catalog};

const fn operator_name(operator: SetOperator) -> &'static str {
    match operator {
        SetOperator::Union | SetOperator::UnionAll => "UNION",
        SetOperator::Intersect | SetOperator::IntersectAll => "INTERSECT",
        SetOperator::Except | SetOperator::ExceptAll => "EXCEPT",
    }
}

fn enum_of(data_type: &DataType) -> Option<&EnumRef> {
    match data_type.flattened() {
        DataType::Enum(descriptor) => Some(descriptor),
        DataType::Array(element) => match element.flattened() {
            DataType::Enum(descriptor) => Some(descriptor),
            _ => None,
        },
        _ => None,
    }
}

/// Rejects a set operation that pairs two different enum types in one output
/// column. A branch whose types cannot be inferred statically is left to the
/// executor.
///
/// # Errors
///
/// Returns `42846` naming both enum types.
pub(in crate::sql::binder) fn validate(
    select: &SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Some(set) = &select.set else {
        return Ok(());
    };
    let mut left = select.clone();
    left.set = None;
    let (Ok(left), Ok(right)) = (
        super::super::infer_select_schema_with_context(&left, catalog, context),
        super::super::infer_select_schema_with_context(&set.right, catalog, context),
    ) else {
        return Ok(());
    };
    for (left, right) in left.fields.iter().zip(&right.fields) {
        let (Some(left), Some(right)) = (enum_of(&left.data_type), enum_of(&right.data_type))
        else {
            continue;
        };
        if left.oid != right.oid {
            return Err(DiagnosticError::new(
                "42846",
                format!(
                    "{} could not convert type {} to {}",
                    operator_name(set.operator),
                    right.name,
                    left.name
                ),
            )
            .into());
        }
    }
    Ok(())
}
