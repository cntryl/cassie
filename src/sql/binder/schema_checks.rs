//! Canonical Boolean literals in the existing simple comparison CHECK shape.

use super::{AlterTableOperation, CassieError, CollectionSchema, DataType};
use crate::catalog::FieldConstraint;
use crate::sql::ast::FieldDefinition;
use std::collections::HashSet;

/// Binds simple CHECK literals before returning the existing own-constraint
/// snapshot used to resolve CREATE TABLE's uniqueness and foreign keys.
pub(super) fn bind_create_checks(
    fields: &mut [FieldDefinition],
) -> Result<Vec<FieldConstraint>, CassieError> {
    let booleans = boolean_fields(
        fields
            .iter()
            .map(|field| (field.name.as_str(), &field.data_type)),
    );
    for field in fields.iter_mut() {
        bind_checks(&mut field.constraints, &booleans)?;
    }
    Ok(fields
        .iter()
        .flat_map(|field| field.constraints.iter().cloned())
        .collect())
}

/// Runs after ALTER has resolved declared column spelling and validated its
/// targets, before the executor validates stored rows or publishes metadata.
pub(super) fn bind_alter_checks(
    operation: &mut AlterTableOperation,
    schema: &CollectionSchema,
) -> Result<(), CassieError> {
    let mut booleans = boolean_fields(
        schema
            .fields
            .iter()
            .map(|field| (field.name.as_str(), &field.data_type)),
    );
    let constraints = match operation {
        AlterTableOperation::AddConstraint { constraints } => constraints,
        AlterTableOperation::AddColumn {
            field,
            data_type,
            constraints,
        } => {
            if matches!(data_type, DataType::Boolean) {
                booleans.insert(crate::sql::ColumnIdentifierPath::stored_field_key(field));
            }
            constraints
        }
        _ => return Ok(()),
    };
    bind_checks(constraints, &booleans)
}

fn boolean_fields<'a>(fields: impl Iterator<Item = (&'a str, &'a DataType)>) -> HashSet<String> {
    fields
        .filter(|(_, data_type)| matches!(data_type, DataType::Boolean))
        .map(|(name, _)| crate::sql::ColumnIdentifierPath::stored_field_key(name))
        .collect()
}

fn bind_checks(
    constraints: &mut [FieldConstraint],
    booleans: &HashSet<String>,
) -> Result<(), CassieError> {
    for check in constraints
        .iter_mut()
        .filter_map(|constraint| constraint.check.as_mut())
    {
        if !booleans.contains(&crate::sql::ColumnIdentifierPath::stored_field_key(
            &check.field,
        )) && !booleans.contains(&crate::sql::ColumnIdentifierPath::reference_field_key(
            check.field.trim(),
        )) {
            continue;
        }
        let value = match &check.value {
            serde_json::Value::Bool(_) | serde_json::Value::Null => continue,
            serde_json::Value::String(text) => crate::types::boolean::parse_text(text)
                .map(serde_json::Value::Bool)
                .ok_or_else(|| {
                    CassieError::Planner(format!(
                        "invalid input syntax for type boolean in CHECK on '{}': {text:?}",
                        check.field,
                    ))
                })?,
            other => {
                return Err(CassieError::Planner(format!(
                "CHECK comparison for Boolean column '{}' requires a Boolean literal, got {other}",
                check.field,
            )))
            }
        };
        check.value = value;
    }
    Ok(())
}
