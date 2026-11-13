//! Field selection typing for composite operands.
use super::{CassieError, DataType};

/// The type of `field` selected from a value of `operand_type`.
///
/// # Errors
///
/// Returns SQLSTATE `42809` for a non-composite operand and `42703` for an
/// unknown field.
pub(crate) fn composite_field_type(
    operand_type: &DataType,
    field: &str,
) -> Result<DataType, CassieError> {
    if field == crate::sql::ast::FIELD_ACCESS_ALL {
        return Err(crate::app::DiagnosticError::new(
            "0A000",
            "(expression).* expansion is supported only for a column or cast in the SELECT list"
                .to_string(),
        )
        .into());
    }
    let DataType::Composite(descriptor) = operand_type else {
        return Err(crate::app::DiagnosticError::new(
            "42809",
            format!(
                "column notation .{field} applied to type {}, which is not a composite type",
                operand_type.type_name()
            ),
        )
        .into());
    };
    descriptor
        .field_index(field)
        .map(|index| descriptor.fields[index].data_type.clone())
        .ok_or_else(|| {
            crate::app::DiagnosticError::new(
                "42703",
                format!(
                    "column \"{field}\" not found in data type {}",
                    descriptor.name
                ),
            )
            .into()
        })
}

/// Validates a field selection whose operand is a column (or a nested field
/// selection) with a known type, returning the selected field's type.
///
/// # Errors
///
/// Returns the SQLSTATE `42809` / `42703` errors of [`composite_field_type`].
pub(crate) fn validate_field_access(
    function: &crate::sql::ast::FunctionCall,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<DataType>, CassieError> {
    use crate::sql::ast::{Expr, FIELD_ACCESS_FUNCTION};
    if !function.name.eq_ignore_ascii_case(FIELD_ACCESS_FUNCTION) {
        return Ok(None);
    }
    let [operand, Expr::StringLiteral(field)] = function.args.as_slice() else {
        return Ok(None);
    };
    let operand_type = match operand {
        Expr::Column(name) => crate::sql::field_type_for_column(field_types, name).cloned(),
        Expr::Function(inner) => validate_field_access(inner, field_types)?,
        _ => None,
    };
    operand_type
        .filter(|data_type| !matches!(data_type, DataType::Null))
        .map(|data_type| composite_field_type(&data_type, field))
        .transpose()
}
