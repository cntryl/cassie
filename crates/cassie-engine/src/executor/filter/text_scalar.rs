//! Bounded scalar text inspection and signed prefix/suffix selection.

use super::text_selection::text;
use super::{require_arg_count_range, signed_integer_arg, QueryError, Value};
use crate::app::CassieError;

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    let range = match name {
        "length" | "len" | "char_length" | "character_length" | "octet_length" | "bit_length"
        | "ascii" | "chr" => 1..=1,
        "left" | "right" | "starts_with" => 2..=2,
        "num_nulls" | "num_nonnulls" => 1..=usize::MAX,
        _ => return None,
    };
    Some((|| {
        require_arg_count_range(name, args, range)?;
        if matches!(name, "num_nulls" | "num_nonnulls") {
            let nulls = args
                .iter()
                .filter(|value| matches!(value, Value::Null))
                .count();
            return integer(if name == "num_nulls" {
                nulls
            } else {
                args.len() - nulls
            });
        }
        if args.iter().any(|value| matches!(value, Value::Null)) {
            return Ok(Value::Null);
        }
        if name == "chr" {
            return character(args);
        }
        let source = text(name, &args[0])?;
        match name {
            "left" | "right" => signed_slice(name, source, args),
            "starts_with" => Ok(Value::Bool(source.starts_with(text(name, &args[1])?))),
            "octet_length" => integer(source.len()),
            "bit_length" => integer(source.len().checked_mul(8).ok_or_else(integer_overflow)?),
            "ascii" => Ok(Value::Int64(
                source
                    .chars()
                    .next()
                    .map_or(0, |character| i64::from(u32::from(character))),
            )),
            _ => integer(source.chars().count()),
        }
    })())
}

fn integer(value: usize) -> Result<Value, QueryError> {
    i32::try_from(value)
        .map(|value| Value::Int64(i64::from(value)))
        .map_err(|_| integer_overflow())
}

fn integer_overflow() -> QueryError {
    CassieError::Execution("integer out of range".into()).into()
}

fn character(args: &[Value]) -> Result<Value, QueryError> {
    let codepoint = signed_integer_arg("chr", &args[0])?.expect("strict argument");
    if codepoint == 0 {
        return Err(CassieError::ResourceLimit("null character not permitted".into()).into());
    }
    if codepoint < 0 {
        return Err(
            CassieError::InvalidParameterValue("character number must be positive".into()).into(),
        );
    }
    let character = u32::try_from(codepoint)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| {
            CassieError::ResourceLimit("requested character not valid for encoding".into())
        })?;
    Ok(Value::String(character.to_string()))
}

fn signed_slice(name: &str, source: &str, args: &[Value]) -> Result<Value, QueryError> {
    let count = i128::from(signed_integer_arg(name, &args[1])?.expect("strict argument"));
    let length = source.chars().count();
    let total = i128::try_from(length).unwrap_or(i128::MAX);
    let (start, end) = match (name, count >= 0) {
        ("left", true) => (0, count.min(total)),
        ("left", false) => (0, (total + count).max(0)),
        (_, true) => ((total - count).max(0), total),
        // REL_18_6 text_right negates int32 before clipping. INT_MIN therefore
        // selects offset zero; preserve the pinned oracle without Rust overflow.
        (_, false) if count == i128::from(i32::MIN) => (0, total),
        (_, false) => ((-count).min(total), total),
    };
    let boundary = |index| {
        source
            .char_indices()
            .nth(usize::try_from(index).unwrap_or(usize::MAX))
            .map_or(source.len(), |(offset, _)| offset)
    };
    Ok(Value::String(
        source[boundary(start)..boundary(end)].to_owned(),
    ))
}
