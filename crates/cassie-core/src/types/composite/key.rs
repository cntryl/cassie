//! Order- and equality-preserving keys for record values.
//!
//! PostgreSQL's `record_cmp` compares field by field in declaration order,
//! treats two NULL fields as equal and sorts a NULL field after any value.
use super::RecordValue;
use std::io::{self, Write};
use std::mem::size_of;

use crate::types::typed::TypedKey;
use crate::types::typed::TypedValue;
use crate::types::{extended::numeric::NumericKey, Value};

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
    Array(ArrayKey),
    Null,
}

/// PostgreSQL compares elements, count, rank, all lengths, then all lower bounds.
/// Vec's element order includes its length when one element sequence is a prefix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ArrayKey {
    elements: Vec<FieldKey>,
    rank: usize,
    lengths: Vec<i32>,
    lower_bounds: Vec<i32>,
}

impl RecordKey {
    /// Owned key backing; the descriptor and source value are separate owners.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.0.iter().fold(
            self.0.capacity().saturating_mul(size_of::<FieldKey>()),
            |bytes, field| bytes.saturating_add(field.retained_bytes()),
        )
    }

    /// Bound key backing and conversion scratch before field keys are constructed.
    #[must_use]
    pub fn allocation_bound(record: &RecordValue) -> usize {
        record.fields().iter().fold(
            record.fields().len().saturating_mul(size_of::<FieldKey>()),
            |bytes, value| bytes.saturating_add(field_bound(value)),
        )
    }
}

impl FieldKey {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Text(text) => text.capacity(),
            Self::Typed(TypedKey::Record(record)) => record.retained_bytes(),
            Self::Typed(TypedKey::Bits(bytes, _)) => bytes.capacity(),
            Self::Typed(TypedKey::Numeric(NumericKey::Finite { mantissa, .. })) => {
                numeric_bytes(mantissa.bits())
            }
            Self::Array(array) => array.elements.iter().fold(
                array
                    .elements
                    .capacity()
                    .saturating_mul(size_of::<Self>())
                    .saturating_add(
                        array
                            .lengths
                            .capacity()
                            .saturating_add(array.lower_bounds.capacity())
                            .saturating_mul(size_of::<i32>()),
                    ),
                |bytes, field| bytes.saturating_add(field.retained_bytes()),
            ),
            Self::Bool(_) | Self::Int(_) | Self::Float(_) | Self::Typed(_) | Self::Null => 0,
        }
    }
}

fn numeric_bytes(bits: u64) -> usize {
    // Clone/normalization retains whole native limbs, including spare capacity.
    usize::try_from(bits.div_ceil(64))
        .unwrap_or(usize::MAX)
        .saturating_mul(16)
        .saturating_add(32)
}

fn field_bound(value: &Value) -> usize {
    match value {
        Value::String(text) => text.len(),
        Value::Typed(TypedValue::Record(record)) => RecordKey::allocation_bound(record),
        Value::Typed(TypedValue::Numeric(crate::types::extended::numeric::Numeric::Finite {
            mantissa,
            ..
        })) => numeric_bytes(mantissa.bits()).saturating_mul(2),
        Value::Typed(typed) => typed.heap_bytes(),
        Value::Array(array) => array.elements().iter().fold(
            array
                .elements()
                .len()
                .saturating_mul(size_of::<FieldKey>())
                .saturating_add(
                    array
                        .dimensions()
                        .len()
                        .saturating_mul(2 * size_of::<i32>()),
                ),
            |bytes, value| bytes.saturating_add(field_bound(value)),
        ),
        // Formatting and the final join coexist with one string per float.
        Value::Vector(vector) => vector
            .values
            .len()
            .saturating_mul(2 * 351 + size_of::<String>())
            .saturating_add(1_024),
        Value::Json(document) => {
            let mut count = JsonBytes(0);
            if serde_json::to_writer(&mut count, document).is_err() {
                return usize::MAX;
            }
            // serde_json's buffer starts at128 and grows geometrically.
            count.0.saturating_mul(2).max(128)
        }
        Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => 0,
    }
}

struct JsonBytes(usize);

impl Write for JsonBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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
        Value::Array(array) => FieldKey::Array(ArrayKey {
            elements: array.elements().iter().map(field_key).collect(),
            rank: array.dimensions().len(),
            lengths: array
                .dimensions()
                .iter()
                .map(|dimension| dimension.length)
                .collect(),
            lower_bounds: array
                .dimensions()
                .iter()
                .map(|dimension| dimension.lower_bound)
                .collect(),
        }),
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
