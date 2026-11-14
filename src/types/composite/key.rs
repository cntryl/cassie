//! Order- and equality-preserving keys for record values.
//!
//! PostgreSQL's `record_cmp` compares field by field in declaration order,
//! treats two NULL fields as equal and sorts a NULL field after any value.
use super::RecordValue;
use crate::types::typed::TypedKey;
use crate::types::Value;

/// A record's btree key: one ordered key per field, compared field by field.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordKey(Vec<FieldKey>);

/// The key of one field. Variants of one declared field type always agree, so
/// the variant order only decides how a NULL compares: after every value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum FieldKey {
    Bool(bool),
    Int(i64),
    /// `float8` bits arranged in PostgreSQL order (see [`float_order_bits`]).
    Float(u64),
    Text(String),
    /// A typed field keeps its own ordered key, so `numeric`, `date` and the
    /// other extended families order by value and not by their spelling.
    Typed(TypedKey),
    Null,
}

/// The key whose order is the record's btree order.
#[must_use]
pub fn record_key(record: &RecordValue) -> RecordKey {
    RecordKey(record.fields().iter().map(field_key).collect())
}

fn field_key(value: &Value) -> FieldKey {
    match value {
        Value::Null => FieldKey::Null,
        Value::Bool(flag) => FieldKey::Bool(*flag),
        Value::Int64(number) => FieldKey::Int(*number),
        Value::Float64(number) => FieldKey::Float(float_order_bits(*number)),
        Value::String(text) => FieldKey::Text(text.clone()),
        Value::Typed(typed) => FieldKey::Typed(typed.key()),
        Value::Vector(vector) => FieldKey::Text(
            vector
                .values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        Value::Array(array) => FieldKey::Text(array.to_nested_json().to_string()),
        Value::Json(document) => FieldKey::Text(document.to_string()),
    }
}

/// PostgreSQL `float8` order: -0 equals 0 and NaN sorts last.
fn float_order_bits(number: f64) -> u64 {
    let number = if number == 0.0 {
        0.0
    } else if number.is_nan() {
        f64::NAN
    } else {
        number
    };
    let bits = number.to_bits();
    if bits >> 63 == 1 {
        !bits
    } else {
        bits | (1 << 63)
    }
}
