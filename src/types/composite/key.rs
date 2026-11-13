//! Order- and equality-preserving keys for record values.
//!
//! PostgreSQL's `record_cmp` compares field by field in declaration order,
//! treats two NULL fields as equal and sorts a NULL field after any value.
use super::RecordValue;
use crate::types::typed::TypedValue;
use crate::types::Value;

/// A byte key whose lexicographic order is the record's btree order.
#[must_use]
pub fn record_key(record: &RecordValue) -> Vec<u8> {
    let mut out = Vec::new();
    for field in record.fields() {
        push_value(&mut out, field);
    }
    out
}

fn push_value(out: &mut Vec<u8>, value: &Value) {
    if value.is_null() {
        out.push(1);
        return;
    }
    out.push(0);
    match value {
        Value::Null => {}
        Value::Bool(flag) => out.push(u8::from(*flag)),
        Value::Int64(number) => {
            out.extend_from_slice(&(number.cast_unsigned() ^ (1 << 63)).to_be_bytes());
        }
        Value::Float64(number) => out.extend_from_slice(&float_order_bits(*number).to_be_bytes()),
        Value::String(text) => push_escaped(out, text.as_bytes()),
        Value::Typed(TypedValue::Record(nested)) => push_escaped(out, &record_key(nested)),
        Value::Typed(typed) => push_escaped(out, format!("{:?}", typed.key()).as_bytes()),
        Value::Vector(vector) => push_escaped(
            out,
            vector
                .values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
                .as_bytes(),
        ),
        Value::Array(array) => push_escaped(out, array.to_nested_json().to_string().as_bytes()),
        Value::Json(document) => push_escaped(out, document.to_string().as_bytes()),
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

/// Byte strings compare correctly when 0x00 is escaped and a terminator ends them.
fn push_escaped(out: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        out.push(*byte);
        if *byte == 0 {
            out.push(0xFF);
        }
    }
    out.extend_from_slice(&[0, 1]);
}
