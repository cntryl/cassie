//! Binds column `DEFAULT` clauses against the column's declared type.
//!
//! A literal default is coerced to the declared type (so `BOOLEAN DEFAULT
//! 'f'` stores `false`), and a volatile default such as `now()` must produce
//! a value the column can hold. A default the column could never accept is
//! rejected here, before any metadata is written.

use super::{AlterTableOperation, CassieError, CollectionSchema};
use crate::catalog::FieldConstraint;
use crate::types::DataType;

pub(super) fn bind_constraint_defaults(
    field: &str,
    data_type: &DataType,
    constraints: &mut [FieldConstraint],
) -> Result<(), CassieError> {
    for constraint in constraints {
        bind_column_default(
            field,
            data_type,
            &mut constraint.default_value,
            constraint.default_expression.as_deref(),
            constraint.default_sequence.as_deref(),
        )?;
    }
    Ok(())
}

pub(super) fn bind_column_default(
    field: &str,
    data_type: &DataType,
    default_value: &mut Option<serde_json::Value>,
    default_expression: Option<&str>,
    default_sequence: Option<&str>,
) -> Result<(), CassieError> {
    if default_sequence.is_some() {
        return Ok(());
    }
    if let Some(expression) = default_expression
        .filter(|expression| crate::sql::definition_guard::null_safe_default(expression))
    {
        *default_value = Some(fold_null_safe_default(expression)?);
    }
    if let Some(value) = default_value.take() {
        let coerced =
            crate::catalog::coerce_default_literal(data_type, value).map_err(|message| {
                CassieError::Planner(format!("invalid DEFAULT for column '{field}': {message}"))
            })?;
        *default_value = Some(coerced);
        return Ok(());
    }
    if let Some(expression) = default_expression {
        if !crate::catalog::volatile_default_accepts(expression, data_type) {
            return Err(CassieError::Planner(format!(
                "DEFAULT {expression} cannot be used for column '{field}' of type {}",
                data_type.type_name()
            )));
        }
    }
    Ok(())
}

/// Binds the default carried by `ALTER TABLE ... ADD COLUMN` or
/// `ALTER COLUMN ... SET DEFAULT` against the column's declared type.
pub(super) fn bind_alter_defaults(
    operation: &mut AlterTableOperation,
    schema: &CollectionSchema,
) -> Result<(), CassieError> {
    match operation {
        AlterTableOperation::AddColumn {
            field,
            data_type,
            constraints,
            ..
        } => bind_constraint_defaults(field, data_type, constraints),
        AlterTableOperation::AlterColumnSetDefault {
            field,
            default_value,
            default_expression,
            default_sequence,
        } => {
            let Some(declared) = schema.fields.iter().find(|entry| {
                crate::sql::ColumnIdentifierPath::stored_field_key(&entry.name)
                    == crate::sql::ColumnIdentifierPath::reference_field_key(field.trim())
            }) else {
                return Ok(());
            };
            bind_column_default(
                field,
                &declared.data_type,
                default_value,
                default_expression.as_deref(),
                default_sequence.as_deref(),
            )
        }
        _ => Ok(()),
    }
}

/// A literal-only query has no catalog names or effects; reuse the complete
/// selected expression binder so unknown literals and mixed inputs follow SQL.
fn fold_null_safe_default(expression: &str) -> Result<serde_json::Value, CassieError> {
    use crate::sql::ast::{QueryStatement, SelectItem};
    let parsed =
        crate::sql::parser::parse_statement(&format!("SELECT {expression} AS default_value"))
            .map_err(|error| CassieError::InvalidQuery(error.to_string()))?;
    let bound = crate::sql::binder::bind(parsed, &crate::catalog::Catalog::default())?;
    let QueryStatement::Select(select) = bound.statement.statement else {
        return Err(CassieError::InvalidQuery(
            "DEFAULT must bind a scalar constant".into(),
        ));
    };
    let Some(SelectItem::Expr { expr, .. }) = select.projection.first() else {
        return Err(CassieError::InvalidQuery(
            "DEFAULT must bind a scalar constant".into(),
        ));
    };
    let value = crate::executor::filter::evaluate_expr_value(
        &crate::executor::batch::BatchRow::new(Vec::new()),
        expr,
        &[],
        None,
        &std::collections::HashMap::new(),
        None,
        None,
    )
    .map_err(CassieError::from)?;
    let crate::types::Value::Bool(value) = value else {
        return Err(CassieError::InvalidQuery(
            "null-safe DEFAULT must produce BOOLEAN".into(),
        ));
    };
    Ok(serde_json::Value::Bool(value))
}
