//! Binds the literal of a simple CHECK on a named enum column (#827).
//!
//! The parser stores the right-hand side as text. A stored enum value is its
//! member id, so the literal is resolved to the member's storage form when the
//! DDL is bound: an unknown label is `22P02` before any metadata is written,
//! and a later `RENAME VALUE` keeps the constraint on the same member.

use crate::app::{CassieError, DiagnosticError};
use crate::catalog::{ConstraintCheck, ConstraintOperator, FieldConstraint};
use crate::types::{DataType, EnumRef, EnumValue, FieldSchema};

pub(super) fn bind(
    constraints: &mut [FieldConstraint],
    fields: &[FieldSchema],
) -> Result<(), CassieError> {
    for check in constraints
        .iter_mut()
        .filter_map(|constraint| constraint.check.as_mut())
    {
        let Some(descriptor) = enum_field(fields, &check.field) else {
            continue;
        };
        bind_check(check, descriptor)?;
    }
    Ok(())
}

fn enum_field<'a>(fields: &'a [FieldSchema], reference: &str) -> Option<&'a EnumRef> {
    fields
        .iter()
        .find(|field| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(reference, &field.name)
        })
        .and_then(|field| match field.data_type.flattened() {
            DataType::Enum(descriptor) => Some(descriptor),
            _ => None,
        })
}

fn operator_symbol(operator: &ConstraintOperator) -> &'static str {
    match operator {
        ConstraintOperator::Eq => "=",
        ConstraintOperator::NotEq => "<>",
        ConstraintOperator::Lt => "<",
        ConstraintOperator::Lte => "<=",
        ConstraintOperator::Gt => ">",
        ConstraintOperator::Gte => ">=",
        ConstraintOperator::Like => "~~",
        ConstraintOperator::IsDistinctFrom => "IS DISTINCT FROM",
        ConstraintOperator::IsNotDistinctFrom => "IS NOT DISTINCT FROM",
    }
}

fn no_operator(descriptor: &EnumRef, operator: &ConstraintOperator, right: &str) -> CassieError {
    DiagnosticError::new(
        "42883",
        format!(
            "operator does not exist: {} {} {right}",
            descriptor.name,
            operator_symbol(operator)
        ),
    )
    .into()
}

fn bind_check(check: &mut ConstraintCheck, descriptor: &EnumRef) -> Result<(), CassieError> {
    if check.operator == ConstraintOperator::Like {
        return Err(no_operator(descriptor, &check.operator, "unknown"));
    }
    let bound = match &check.value {
        serde_json::Value::Null => return Ok(()),
        serde_json::Value::String(label) => EnumValue::parse_label(descriptor, label)
            .map_err(|error| {
                CassieError::from(crate::executor::filter::enum_cast::input_diagnostic(&error))
            })?
            .to_storage_json(),
        stored if descriptor.has_storage_member(stored) => return Ok(()),
        serde_json::Value::Bool(_) => {
            return Err(no_operator(descriptor, &check.operator, "boolean"));
        }
        serde_json::Value::Number(number) if number.is_i64() || number.is_u64() => {
            return Err(no_operator(descriptor, &check.operator, "integer"));
        }
        _ => return Err(no_operator(descriptor, &check.operator, "numeric")),
    };
    check.value = bound;
    // The member comparison is evaluated from the storage form; a bound text
    // expression would compare the label instead.
    check.bound_expression = None;
    Ok(())
}

#[cfg(test)]
#[path = "schema_enum_checks_tests.rs"]
mod tests;
