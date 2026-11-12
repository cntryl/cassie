//! The record value carrier.
use serde::{Deserialize, Serialize};

use super::{CompositeRef, CompositeType};
use crate::types::Value;

/// A non-NULL record: its descriptor and one value per descriptor field.
///
/// SQL NULL is `Value::Null`, so a record whose fields are all NULL stays
/// distinct from a NULL record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordValue {
    descriptor: CompositeRef,
    fields: Vec<Value>,
}

impl RecordValue {
    /// # Errors
    ///
    /// Returns a message when the field count differs from the descriptor.
    pub fn new(descriptor: CompositeRef, fields: Vec<Value>) -> Result<Self, String> {
        if descriptor.fields.len() != fields.len() {
            return Err(format!(
                "record of type {} expects {} fields but received {}",
                descriptor.name,
                descriptor.fields.len(),
                fields.len()
            ));
        }
        Ok(Self { descriptor, fields })
    }

    /// An anonymous record whose descriptor is derived from the field values.
    #[must_use]
    pub fn anonymous(fields: Vec<(Value, crate::types::DataType)>) -> Self {
        let (values, types): (Vec<_>, Vec<_>) = fields.into_iter().unzip();
        Self {
            descriptor: CompositeRef::new(CompositeType::anonymous(types)),
            fields: values,
        }
    }

    #[must_use]
    pub const fn descriptor(&self) -> &CompositeRef {
        &self.descriptor
    }

    #[must_use]
    pub fn fields(&self) -> &[Value] {
        &self.fields
    }

    #[must_use]
    pub fn into_fields(self) -> Vec<Value> {
        self.fields
    }

    /// The value of the field called `name`.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&Value> {
        self.descriptor
            .field_index(name)
            .and_then(|index| self.fields.get(index))
    }

    /// Whether every field is SQL NULL (`record IS NULL` is true for such a value).
    #[must_use]
    pub fn all_fields_null(&self) -> bool {
        self.fields.iter().all(Value::is_null)
    }

    /// Whether no field is SQL NULL (`record IS NOT NULL`).
    #[must_use]
    pub fn no_field_null(&self) -> bool {
        self.fields.iter().all(|field| !field.is_null())
    }

    /// Heap bytes a clone retains, for query memory accounting.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.fields.iter().fold(
            self.fields.capacity().saturating_mul(size_of::<Value>()),
            |bytes, field| bytes.saturating_add(value_heap_bytes(field)),
        )
    }
}

fn value_heap_bytes(value: &Value) -> usize {
    match value {
        Value::String(text) => text.capacity(),
        Value::Typed(typed) => typed.heap_bytes(),
        Value::Vector(vector) => vector.values.capacity().saturating_mul(size_of::<f32>()),
        Value::Array(array) => array
            .elements()
            .iter()
            .fold(0, |bytes, element| {
                bytes.saturating_add(value_heap_bytes(element))
            })
            .saturating_add(array.elements().len().saturating_mul(size_of::<Value>())),
        Value::Json(document) => document.to_string().len(),
        Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => 0,
    }
}

/// Best-effort SQL type of a runtime value, used to describe the fields of an
/// anonymous `ROW(...)`. Untyped NULL and arrays of NULLs report `text`.
#[must_use]
pub fn value_data_type(value: &Value) -> crate::types::DataType {
    use crate::types::DataType;
    match value {
        Value::Null | Value::String(_) => DataType::Text,
        Value::Bool(_) => DataType::Boolean,
        Value::Int64(_) => DataType::BigInt,
        Value::Float64(_) => DataType::Float,
        Value::Vector(_) => DataType::Vector(None),
        Value::Json(_) => DataType::Jsonb,
        Value::Array(array) => DataType::Array(Box::new(
            array
                .elements()
                .iter()
                .find(|element| !matches!(element, Value::Null))
                .map_or(DataType::Text, value_data_type),
        )),
        Value::Typed(typed) => typed.data_type(),
    }
}
