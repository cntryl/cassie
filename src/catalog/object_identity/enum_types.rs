//! `pg_type.oid` of enum column types (#827). `regtype` input and output for
//! enum types is shared with the other user type families in `user_types`.

/// The `pg_type.oid` of an enum column type (its array type for an enum array
/// column), or `None` for any other type.
pub(crate) fn enum_column_type_oid(data_type: &crate::types::DataType) -> Option<i64> {
    use crate::types::DataType;
    match data_type {
        DataType::Enum(descriptor) => Some(i64::from(descriptor.oid)),
        DataType::Array(inner) => match inner.as_ref() {
            DataType::Enum(descriptor) => Some(i64::from(descriptor.array_oid)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
#[path = "enum_types_tests.rs"]
mod tests;
