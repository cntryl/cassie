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
