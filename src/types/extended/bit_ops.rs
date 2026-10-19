//! Operators and functions over `bit` and `bit varying`, and `pg_lsn`
//! arithmetic.
use super::bit_string::BitString;
use super::numeric::Numeric;
use super::{pg_lsn, ExtendedType, TypeInputError, TypedOperator};
use crate::types::typed::TypedValue;
use crate::types::{DataType, Value};

const fn is_bits(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::Extended(ExtendedType::Bit { .. } | ExtendedType::VarBit { .. })
    )
}

const fn is_integer(kind: &DataType) -> bool {
    matches!(kind, DataType::SmallInt | DataType::Int | DataType::BigInt)
}

/// `pg_lsn ± numeric`; integers convert implicitly, floats do not.
const fn is_offset(kind: &DataType) -> bool {
    is_integer(kind) || matches!(kind, DataType::Extended(ExtendedType::Numeric(_)))
}

const NUMERIC: DataType = DataType::Extended(ExtendedType::Numeric(None));

const BIT: DataType = DataType::Extended(ExtendedType::Bit { length: None });
const VARBIT: DataType = DataType::Extended(ExtendedType::VarBit { length: None });
const LSN: DataType = DataType::Extended(ExtendedType::PgLsn);

fn bits(value: &Value) -> Option<&BitString> {
    match value {
        Value::Typed(TypedValue::Bit(bits) | TypedValue::VarBit(bits)) => Some(bits),
        _ => None,
    }
}

fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Int64(value) => Some(*value),
        _ => None,
    }
}

fn number(value: &Value) -> Option<Numeric> {
    match value {
        Value::Int64(value) => Some(Numeric::from_i64(*value)),
        Value::Typed(TypedValue::Numeric(value)) => Some(value.clone()),
        _ => None,
    }
}

fn exact(value: Numeric) -> Value {
    Value::Typed(TypedValue::Numeric(value))
}

fn lsn(value: &Value) -> Option<u64> {
    match value {
        Value::Typed(TypedValue::PgLsn(value)) => Some(*value),
        _ => None,
    }
}

fn bit(value: BitString) -> Value {
    Value::Typed(TypedValue::Bit(value))
}

/// The result type of a bit-string or `pg_lsn` operator.
#[must_use]
pub fn operator_type(op: TypedOperator, left: &DataType, right: &DataType) -> Option<DataType> {
    let lsn = |kind: &DataType| kind == &LSN;
    match op {
        TypedOperator::Concat if is_bits(left) && is_bits(right) => Some(VARBIT),
        TypedOperator::And | TypedOperator::Or | TypedOperator::Xor
            if is_bits(left) && is_bits(right) =>
        {
            Some(BIT)
        }
        TypedOperator::ShiftLeft | TypedOperator::ShiftRight
            if is_bits(left) && is_integer(right) =>
        {
            Some(BIT)
        }
        TypedOperator::Sub if lsn(left) && lsn(right) => Some(NUMERIC),
        TypedOperator::Add | TypedOperator::Sub if lsn(left) && is_offset(right) => Some(LSN),
        TypedOperator::Add if is_offset(left) && lsn(right) => Some(LSN),
        _ => None,
    }
}

/// Evaluates a bit-string or `pg_lsn` operator over non-NULL operands.
#[must_use]
pub fn operator(
    op: TypedOperator,
    left: &Value,
    right: &Value,
) -> Option<Result<Value, TypeInputError>> {
    if let (Some(left), Some(right)) = (bits(left), bits(right)) {
        return match op {
            TypedOperator::Concat => Some(Ok(Value::Typed(TypedValue::VarBit(left.concat(right))))),
            TypedOperator::And => Some(left.combine(right, "AND", |a, b| a & b).map(bit)),
            TypedOperator::Or => Some(left.combine(right, "OR", |a, b| a | b).map(bit)),
            TypedOperator::Xor => Some(left.combine(right, "XOR", |a, b| a ^ b).map(bit)),
            _ => None,
        };
    }
    if let (Some(value), Some(count)) = (bits(left), integer(right)) {
        return match op {
            TypedOperator::ShiftLeft => Some(Ok(bit(value.shift_left(count)))),
            TypedOperator::ShiftRight => Some(Ok(bit(value.shift_left(count.saturating_neg())))),
            _ => None,
        };
    }
    lsn_operator(op, left, right)
}

fn lsn_operator(
    op: TypedOperator,
    left: &Value,
    right: &Value,
) -> Option<Result<Value, TypeInputError>> {
    let wrap = |result: Result<u64, TypeInputError>| {
        Some(result.map(|value| Value::Typed(TypedValue::PgLsn(value))))
    };
    match (op, lsn(left), lsn(right)) {
        (TypedOperator::Sub, Some(left), Some(right)) => {
            Some(Ok(exact(pg_lsn::difference(left, right))))
        }
        (TypedOperator::Add, Some(value), None) => wrap(pg_lsn::offset(value, &number(right)?)),
        (TypedOperator::Sub, Some(value), None) => wrap(pg_lsn::offset(
            value,
            &super::numeric::math::negate(&number(right)?),
        )),
        (TypedOperator::Add, None, Some(value)) => wrap(pg_lsn::offset(value, &number(left)?)),
        _ => None,
    }
}

/// `~` over a bit string.
#[must_use]
pub fn complement(value: &TypedValue) -> Option<TypedValue> {
    match value {
        TypedValue::Bit(bits) | TypedValue::VarBit(bits) => {
            Some(TypedValue::Bit(bits.complement()))
        }
        _ => None,
    }
}

/// The result type of a bit-string or `pg_lsn` function.
#[must_use]
pub fn function_type(name: &str, args: &[DataType]) -> Option<DataType> {
    let first = args.first()?;
    if name == "pg_lsn" && args.len() == 1 && is_offset(first) {
        return Some(LSN);
    }
    if name == "pg_wal_lsn_diff" && args.len() == 2 && first == &LSN {
        return Some(NUMERIC);
    }
    if name == "position" && args.len() == 2 && is_bits(first) {
        return Some(DataType::Int);
    }
    if !is_bits(first) {
        return None;
    }
    match (name, args.len()) {
        ("length" | "bit_length" | "octet_length", 1) | ("get_bit", 2) => Some(DataType::Int),
        ("bit_count", 1) => Some(DataType::BigInt),
        ("substring" | "substr", 2 | 3) | ("overlay", 3 | 4) | ("set_bit", 3) => Some(BIT),
        _ => None,
    }
}

/// Evaluates a bit-string or `pg_lsn` function over non-NULL arguments.
#[must_use]
pub fn function(name: &str, args: &[Value]) -> Option<Result<Value, TypeInputError>> {
    let int = |index: usize| args.get(index).and_then(integer);
    match (name, args) {
        ("pg_lsn", [value]) if lsn(value).is_none() => Some(
            pg_lsn::from_number(&number(value)?)
                .map(|value| Value::Typed(TypedValue::PgLsn(value))),
        ),
        ("pg_wal_lsn_diff", [left, right]) => {
            Some(Ok(exact(pg_lsn::difference(lsn(left)?, lsn(right)?))))
        }
        ("position", [haystack, needle]) => {
            Some(Ok(Value::Int64(bits(haystack)?.position(bits(needle)?))))
        }
        ("length" | "bit_length", [value]) => {
            Some(Ok(Value::Int64(i64::try_from(bits(value)?.len()).ok()?)))
        }
        ("octet_length", [value]) => Some(Ok(Value::Int64(
            i64::try_from(bits(value)?.bytes().len()).ok()?,
        ))),
        ("bit_count", [value]) => Some(Ok(Value::Int64(bits(value)?.count_ones()))),
        ("get_bit", [value, _]) => Some(bits(value)?.get_bit(int(1)?).map(Value::Int64)),
        ("set_bit", [value, _, _]) => Some(bits(value)?.set_bit(int(1)?, int(2)?).map(bit)),
        ("substring" | "substr", [value, _]) => {
            Some(bits(value)?.substring(int(1)?, None).map(bit))
        }
        ("substring" | "substr", [value, _, _]) => {
            Some(bits(value)?.substring(int(1)?, Some(int(2)?)).map(bit))
        }
        ("overlay", [value, replacement, _]) => Some(
            bits(value)?
                .overlay(bits(replacement)?, int(2)?, None)
                .map(bit),
        ),
        ("overlay", [value, replacement, _, _]) => Some(
            bits(value)?
                .overlay(bits(replacement)?, int(2)?, Some(int(3)?))
                .map(bit),
        ),
        _ => None,
    }
}
