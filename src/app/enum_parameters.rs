//! Bind parameters whose declared or described type is a named enum or an
//! enum array (#827).
//!
//! The wire layers deliver a label (or an array of labels) as text. A
//! parameter parses it against the current descriptor, which owns `22P02` for
//! an unknown label and `55P04` for a label added in the open transaction.
use super::{CassieError, DiagnosticError};
use crate::catalog::Catalog;
use crate::executor::EnumColumn;
use crate::types::enum_type::{EnumInputError, EnumValue};
use crate::types::typed::TypedValue;
use crate::types::{DataType, EnumRef, SqlArray, Value};

/// The enum type a parameter OID names.
#[derive(Debug, Clone)]
pub(crate) struct EnumParameter {
    descriptor: EnumRef,
    is_array: bool,
}

impl EnumParameter {
    /// The enum (or enum array) type `oid` names in `database`, if any.
    #[must_use]
    pub(crate) fn for_oid(catalog: &Catalog, database: &str, oid: i32) -> Option<Self> {
        match crate::sql::binder::parameter_data_type_for_oid_in_database(oid, catalog, database)? {
            DataType::Enum(descriptor) => Some(Self {
                descriptor,
                is_array: false,
            }),
            DataType::Array(inner) => match *inner {
                DataType::Enum(descriptor) => Some(Self {
                    descriptor,
                    is_array: true,
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// The wire description pgwire decodes binary input with.
    #[must_use]
    pub(crate) fn column(&self) -> EnumColumn {
        EnumColumn {
            element_oid: self.descriptor.oid,
            is_array: self.is_array,
        }
    }

    /// Converts a decoded label (text, or an array of labels) to a typed enum
    /// value.
    ///
    /// # Errors
    ///
    /// `22P02` for an unknown label or malformed array text and `55P04` for a
    /// label that is not committed yet.
    pub(crate) fn coerce(&self, value: Value) -> Result<Value, CassieError> {
        match value {
            Value::Null => Ok(Value::Null),
            Value::Typed(typed) if typed.as_enum().is_some() => Ok(Value::Typed(typed)),
            Value::String(text) if self.is_array => self.array_from_text(&text),
            Value::String(text) => self.scalar(&text),
            Value::Array(array) if self.is_array => self.array(&array),
            _ => Err(DiagnosticError::new(
                "22P02",
                format!("invalid input value for enum {}", self.descriptor.name),
            )
            .into()),
        }
    }

    fn scalar(&self, label: &str) -> Result<Value, CassieError> {
        EnumValue::parse_label(&self.descriptor, label)
            .map(|value| Value::Typed(TypedValue::Enum(value)))
            .map_err(|error| input_error(&error))
    }

    fn array_from_text(&self, text: &str) -> Result<Value, CassieError> {
        let malformed = |detail: String| -> CassieError {
            DiagnosticError::new("22P02", format!("malformed array literal: \"{text}\""))
                .with_detail(detail)
                .into()
        };
        let parsed = crate::types::array::parse_text_array_with_dimensions(text, &DataType::Text)
            .map_err(|error| malformed(error.to_string()))?;
        let array = parsed.into_sql_array(&DataType::Text).map_err(malformed)?;
        self.array(&array)
    }

    fn array(&self, array: &SqlArray) -> Result<Value, CassieError> {
        let elements = array
            .elements()
            .iter()
            .map(|element| match element {
                Value::Null => Ok(Value::Null),
                Value::String(label) => self.scalar(label),
                typed @ Value::Typed(_) => Ok(typed.clone()),
                _ => Err(DiagnosticError::new("22P02", "invalid enum array element").into()),
            })
            .collect::<Result<Vec<_>, CassieError>>()?;
        SqlArray::new(array.dimensions().to_vec(), elements)
            .map(Value::Array)
            .map_err(|message| DiagnosticError::new("22P02", message).into())
    }
}

fn input_error(error: &EnumInputError) -> CassieError {
    let mut diagnostic = DiagnosticError::new(error.sqlstate(), error.message());
    if let Some(hint) = error.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic.into()
}

/// The enum array type OID of a typed enum array parameter, from its first
/// element. `None` for any other array.
#[must_use]
pub(crate) fn array_oid(array: &SqlArray) -> Option<i32> {
    array.elements().iter().find_map(|element| match element {
        Value::Typed(typed) => typed
            .as_enum()
            .and_then(|value| i32::try_from(value.descriptor().array_oid).ok()),
        _ => None,
    })
}

/// Parses every parameter whose declared OID names an enum or enum array
/// into a typed enum value, leaving the others untouched.
///
/// # Errors
///
/// The first parameter's `22P02` or `55P04`.
pub(crate) fn coerce_declared(
    catalog: &Catalog,
    database: &str,
    declared_oids: &[i32],
    params: &mut [Value],
) -> Result<(), CassieError> {
    for (param, oid) in params.iter_mut().zip(declared_oids) {
        if let Some(target) = EnumParameter::for_oid(catalog, database, *oid) {
            *param = target.coerce(std::mem::replace(param, Value::Null))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "enum_parameters_tests.rs"]
mod tests;
