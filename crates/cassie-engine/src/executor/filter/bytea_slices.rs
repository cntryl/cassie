//! The BYTEA overloads of `length`, `octet_length` and `substring`/`substr`.
//!
//! The row carrier is canonical hex (`\x` followed by two digits per byte), so
//! byte counts and byte ranges map directly onto carrier digits without
//! decoding the value.

use super::{signed_integer_arg, QueryError, Value};
use crate::app::CassieError;

/// Whether `name` has a BYTEA overload evaluated here.
pub(super) fn is_overloaded(name: &str) -> bool {
    matches!(name, "length" | "octet_length" | "substring" | "substr")
}

pub(super) fn evaluate(name: &str, args: &[Value]) -> Result<Value, QueryError> {
    let arity = if matches!(name, "length" | "octet_length") {
        1..=1
    } else {
        2..=3
    };
    super::require_arg_count_range(name, args, arity)?;
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let Some(Value::String(carrier)) = args.first() else {
        return Err(QueryError::General(
            "invalid BYTEA function argument".into(),
        ));
    };
    let digits = carrier
        .strip_prefix("\\x")
        .ok_or_else(|| QueryError::General("invalid canonical BYTEA carrier".into()))?;
    let length = digits.len() / 2;
    if matches!(name, "length" | "octet_length") {
        return i32::try_from(length)
            .map(|length| Value::Int64(i64::from(length)))
            .map_err(|_| CassieError::Execution("integer out of range".into()).into());
    }
    let start = i128::from(signed_integer_arg(name, &args[1])?.expect("strict argument"));
    let end = match args.get(2) {
        Some(count) => {
            let count = signed_integer_arg(name, count)?.expect("strict argument");
            if count < 0 {
                return Err(
                    CassieError::Execution("negative substring length not allowed".into()).into(),
                );
            }
            start + i128::from(count)
        }
        None => i128::MAX,
    };
    let clamp = |position: i128| {
        usize::try_from((position - 1).clamp(0, i128::try_from(length).unwrap_or(i128::MAX)))
            .unwrap_or(length)
    };
    let (begin, end) = (clamp(start), clamp(end));
    let slice = digits
        .get(begin * 2..end.max(begin) * 2)
        .unwrap_or_default();
    Ok(Value::String(format!("\\x{slice}")))
}
