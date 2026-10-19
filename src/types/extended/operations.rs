//! Operator, function and comparison dispatch for extended families that
//! define their own SQL operators (network, bit-string and `pg_lsn`).
use std::cmp::Ordering;

use super::{bit_ops, network_ops, ExtendedType, TypeInputError, TypedOperator};
use crate::types::typed::TypedValue;
use crate::types::{DataType, Value};

/// Whether operators over `kind` are selected by this dispatcher.
#[must_use]
pub const fn handles(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::Extended(
            ExtendedType::Inet
                | ExtendedType::Cidr
                | ExtendedType::Macaddr
                | ExtendedType::Macaddr8
                | ExtendedType::PgLsn
                | ExtendedType::Bit { .. }
                | ExtendedType::VarBit { .. }
        )
    )
}

/// Whether `value` belongs to a family this dispatcher handles.
#[must_use]
pub const fn handles_value(value: &TypedValue) -> bool {
    !matches!(value, TypedValue::Real(_) | TypedValue::Numeric(_))
        && !super::temporal::ops::is_temporal(value)
}

/// The result type of `left op right`, or `None` when no extended
/// operator applies.
#[must_use]
pub fn operator_type(op: TypedOperator, left: &DataType, right: &DataType) -> Option<DataType> {
    network_ops::operator_type(op, left, right).or_else(|| bit_ops::operator_type(op, left, right))
}

/// Evaluates `left op right` over non-NULL operands.
#[must_use]
pub fn operator(
    op: TypedOperator,
    left: &Value,
    right: &Value,
) -> Option<Result<Value, TypeInputError>> {
    network_ops::operator(op, left, right).or_else(|| bit_ops::operator(op, left, right))
}

/// Evaluates prefix `~`.
#[must_use]
pub fn complement(value: &TypedValue) -> Option<TypedValue> {
    network_ops::complement(value).or_else(|| bit_ops::complement(value))
}

/// The result type of prefix `~` over `kind`.
#[must_use]
pub const fn complement_type(kind: &DataType) -> Option<DataType> {
    match kind {
        DataType::Extended(ExtendedType::Inet | ExtendedType::Cidr) => {
            Some(DataType::Extended(ExtendedType::Inet))
        }
        DataType::Extended(ExtendedType::Macaddr) => {
            Some(DataType::Extended(ExtendedType::Macaddr))
        }
        DataType::Extended(ExtendedType::Macaddr8) => {
            Some(DataType::Extended(ExtendedType::Macaddr8))
        }
        DataType::Extended(ExtendedType::Bit { .. } | ExtendedType::VarBit { .. }) => {
            Some(DataType::Extended(ExtendedType::Bit { length: None }))
        }
        _ => None,
    }
}

/// Compares two values of one comparison class (`inet`/`cidr`,
/// `bit`/`varbit`, or one type), in PostgreSQL order.
#[must_use]
pub fn compare(left: &TypedValue, right: &TypedValue) -> Option<Ordering> {
    let (left, right) = (left.key(), right.key());
    (std::mem::discriminant(&left) == std::mem::discriminant(&right)).then(|| left.cmp(&right))
}

/// Functions that exist only for these families, so a NULL argument
/// yields NULL without knowing its type.
const STRICT_FUNCTIONS: &[&str] = &[
    "host",
    "masklen",
    "set_masklen",
    "netmask",
    "hostmask",
    "network",
    "broadcast",
    "family",
    "abbrev",
    "inet_same_family",
    "inet_merge",
    "macaddr8_set7bit",
    "pg_wal_lsn_diff",
    "pg_lsn",
];

/// Strict builtins shared with other types; NULL short-circuits only
/// when another argument already selects one of these families.
const SHARED_STRICT_FUNCTIONS: &[&str] = &[
    "length",
    "bit_length",
    "octet_length",
    "bit_count",
    "get_bit",
    "set_bit",
    "substring",
    "substr",
    "overlay",
    "position",
    "trunc",
    "text",
];

/// Whether `name` is a function defined only for these families.
#[must_use]
pub fn is_family_function(name: &str) -> bool {
    let name = local_name(name);
    STRICT_FUNCTIONS.contains(&name.as_str())
}

/// Whether `name` is a builtin evaluated by these families (exclusive or
/// overloaded for them), so validation leaves it to type selection.
#[must_use]
pub fn is_function_name(name: &str) -> bool {
    let name = local_name(name);
    STRICT_FUNCTIONS.contains(&name.as_str())
        || ["trunc", "text", "overlay"].contains(&name.as_str())
}

fn local_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    lower
        .strip_prefix("pg_catalog.")
        .map_or_else(|| lower.clone(), str::to_string)
}

/// The result type of a function over these families.
#[must_use]
pub fn function_type(name: &str, args: &[DataType]) -> Option<DataType> {
    let name = local_name(name);
    network_ops::function_type(&name, args).or_else(|| bit_ops::function_type(&name, args))
}

/// Evaluates a function over these families; `None` when `name` with
/// these arguments is not one of them.
#[must_use]
pub fn function(name: &str, args: &[Value]) -> Option<Result<Value, TypeInputError>> {
    let name = local_name(name);
    let typed = args
        .iter()
        .any(|arg| matches!(arg, Value::Typed(value) if handles_value(value)));
    if args.iter().any(|arg| matches!(arg, Value::Null))
        && (STRICT_FUNCTIONS.contains(&name.as_str())
            || (typed && SHARED_STRICT_FUNCTIONS.contains(&name.as_str())))
    {
        return Some(Ok(Value::Null));
    }
    if !typed && name != "pg_lsn" {
        return None;
    }
    network_ops::function(&name, args).or_else(|| bit_ops::function(&name, args))
}
