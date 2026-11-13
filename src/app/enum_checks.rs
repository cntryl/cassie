//! CHECK constraints on named enum columns (#827).
//!
//! The binder stores the literal of an enum CHECK as the member's storage form,
//! so equality is the generic document comparison. Ordering needs the column's
//! descriptor: members compare by sort order, never by member id or label.

use super::{Cassie, CassieError};
use crate::catalog::{CollectionSchema, ConstraintCheck, ConstraintOperator};
use crate::types::{DataType, EnumRef, EnumValue};

fn enum_descriptor<'a>(schema: Option<&'a CollectionSchema>, field: &str) -> Option<&'a EnumRef> {
    schema?
        .fields
        .iter()
        .find(|candidate| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(field, &candidate.name)
        })
        .and_then(|candidate| match &candidate.data_type {
            DataType::Enum(descriptor) => Some(descriptor),
            _ => None,
        })
}

/// The member a stored document or a legacy text literal names.
fn member(descriptor: &EnumRef, value: &serde_json::Value) -> Option<EnumValue> {
    match value {
        serde_json::Value::String(label) => EnumValue::from_label(descriptor, label),
        stored => EnumValue::storage_member_id(stored)
            .and_then(|member_id| EnumValue::new(descriptor.clone(), member_id)),
    }
}

fn unknown_member(descriptor: &EnumRef, field: &str) -> CassieError {
    CassieError::Execution(format!(
        "CHECK on column '{field}' compares a value that is not a member of enum {}",
        descriptor.name
    ))
}

/// Evaluates `check` for an enum column, or `None` when the generic
/// comparison already has the right semantics.
fn evaluate(
    descriptor: &EnumRef,
    value: &serde_json::Value,
    check: &ConstraintCheck,
) -> Option<Result<bool, CassieError>> {
    if value.is_null() || check.value.is_null() {
        return None;
    }
    let (Some(left), Some(right)) = (member(descriptor, value), member(descriptor, &check.value))
    else {
        return Some(Err(unknown_member(descriptor, &check.field)));
    };
    let order = left.order_key().cmp(&right.order_key());
    Some(Ok(match check.operator {
        ConstraintOperator::Eq | ConstraintOperator::IsNotDistinctFrom => order.is_eq(),
        ConstraintOperator::NotEq | ConstraintOperator::IsDistinctFrom => order.is_ne(),
        ConstraintOperator::Lt => order.is_lt(),
        ConstraintOperator::Lte => order.is_le(),
        ConstraintOperator::Gt => order.is_gt(),
        ConstraintOperator::Gte => order.is_ge(),
        ConstraintOperator::Like => return None,
    }))
}

impl Cassie {
    /// Whether the stored value of `check.field` satisfies `check`, resolving
    /// a named enum column through its descriptor.
    pub(super) fn satisfies_column_check(
        schema: Option<&CollectionSchema>,
        existing: Option<&serde_json::Value>,
        check: &ConstraintCheck,
    ) -> Result<bool, CassieError> {
        let value = existing.unwrap_or(&serde_json::Value::Null);
        let value_is_sql_null = super::field_value_is_sql_null(existing, schema, &check.field);
        if !value_is_sql_null {
            if let Some(result) = enum_descriptor(schema, &check.field)
                .and_then(|descriptor| evaluate(descriptor, value, check))
            {
                return result;
            }
        }
        Self::satisfies_check_constraint(value, check, value_is_sql_null)
    }
}

#[cfg(test)]
#[path = "enum_checks_tests.rs"]
mod tests;
