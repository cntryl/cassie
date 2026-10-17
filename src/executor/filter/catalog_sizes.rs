//! PostgreSQL size units over existing bigint/text carriers.
//! The parser multiplies decimal digits exactly with constant-size scratch.
use super::{require_arg_count, QueryError, Value};
use crate::app::CassieError;
use crate::types::SizeInputError;

const UNITS: [&str; 6] = ["bytes", "kB", "MB", "GB", "TB", "PB"];

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    if !matches!(name, "pg_size_bytes" | "pg_size_pretty") {
        return None;
    }
    Some((|| {
        require_arg_count(name, args, 1)?;
        match (&args[0], name) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::String(input), "pg_size_bytes") => parse(input).map(Value::Int64),
            (Value::Int64(value), "pg_size_pretty") => Ok(Value::String(pretty(*value))),
            _ => Err(CassieError::Unsupported(
                "size helper requires its selected text/bigint signature; NUMERIC formatting remains pending under #793/#817".into(),
            ).into()),
        }
    })())
}

fn pretty(value: i64) -> String {
    // Keep one guard bit until the selected unit, then round away from zero.
    // i128 also makes the negative bigint boundary safe.
    let mut value = i128::from(value);
    for (index, unit) in UNITS.iter().enumerate() {
        let limit = if index == 0 { 10_240 } else { 20_479 };
        if value.abs() < limit || index == UNITS.len() - 1 {
            if index != 0 {
                value = value.midpoint(value.signum());
            }
            return format!("{value} {unit}");
        }
        value /= if index == 0 { 512 } else { 1024 };
    }
    unreachable!("final unit always formats")
}

fn overflow() -> QueryError {
    CassieError::SizeInput(SizeInputError::BigIntOverflow).into()
}
fn numeric_overflow() -> QueryError {
    CassieError::SizeInput(SizeInputError::NumericOverflow).into()
}
fn invalid(input: &str) -> QueryError {
    CassieError::SizeInput(SizeInputError::InvalidSize(input.to_owned())).into()
}
fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn parse(input: &str) -> Result<i64, QueryError> {
    let bytes = input.as_bytes();
    let mut cursor = bytes.iter().take_while(|byte| whitespace(**byte)).count();
    let negative = bytes.get(cursor) == Some(&b'-');
    if matches!(bytes.get(cursor), Some(b'-' | b'+')) {
        cursor += 1;
    }
    let start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    let integer_digits = cursor - start;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
    }
    let mantissa = &bytes[start..cursor];
    let digits = mantissa.iter().filter(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return Err(invalid(input));
    }
    let exponent = exponent(bytes, &mut cursor)?;
    let point = i64::try_from(integer_digits)
        .map_err(|_| numeric_overflow())?
        .checked_add(exponent)
        .ok_or_else(numeric_overflow)?;
    // PostgreSQL NUMERIC's input limits apply before int8 conversion, even to zero.
    let scale = i64::try_from(digits - integer_digits)
        .map_err(|_| numeric_overflow())?
        .checked_sub(exponent)
        .ok_or_else(numeric_overflow)?;
    if scale > 16_383
        || point.saturating_sub(
            i64::try_from(
                mantissa
                    .iter()
                    .filter(|byte| byte.is_ascii_digit())
                    .take_while(|byte| **byte == b'0')
                    .count(),
            )
            .map_err(|_| numeric_overflow())?,
        ) > 131_072
            && mantissa.iter().any(|digit| matches!(digit, b'1'..=b'9'))
    {
        return Err(numeric_overflow());
    }
    let unit = input[cursor..].trim_matches(|character: char| {
        character.is_ascii() && whitespace(u8::try_from(u32::from(character)).unwrap_or(0))
    });
    let unit_index = if unit.is_empty() || unit.eq_ignore_ascii_case("B") {
        0
    } else {
        UNITS
            .iter()
            .position(|name| unit.eq_ignore_ascii_case(name))
            .ok_or_else(|| invalid(input))?
    };
    let multiplier = 1_i128 << (unit_index * 10);
    scaled_integer(mantissa, digits, point, multiplier, negative)
}

fn scaled_integer(
    mantissa: &[u8],
    digits: usize,
    point: i64,
    multiplier: i128,
    negative: bool,
) -> Result<i64, QueryError> {
    let mut integer = 0_i128;
    for (index, digit) in mantissa
        .iter()
        .filter(|byte| byte.is_ascii_digit())
        .enumerate()
    {
        if i64::try_from(index).map_err(|_| numeric_overflow())? >= point {
            break;
        }
        integer = integer
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(digit - b'0')))
            .ok_or_else(overflow)?;
    }
    let zeros = point.saturating_sub(i64::try_from(digits).map_err(|_| numeric_overflow())?);
    if integer != 0 {
        if zeros > 19 {
            return Err(overflow());
        }
        for _ in 0..zeros {
            integer = integer.checked_mul(10).ok_or_else(overflow)?;
        }
    }
    let mut fraction = 0_i128;
    let mut remainder = 0;
    let mut index = digits;
    for digit in mantissa.iter().rev().filter(|byte| byte.is_ascii_digit()) {
        index -= 1;
        if i64::try_from(index).map_err(|_| numeric_overflow())? < point {
            break;
        }
        let product = i128::from(digit - b'0') * multiplier + fraction;
        fraction = product / 10;
        remainder = product % 10;
    }
    // Virtual leading fractional zeroes from a negative exponent. Once this
    // carry is zero an additional zero also clears a possible rounding digit.
    for _ in 0..point.saturating_neg().clamp(0, 17) {
        remainder = fraction % 10;
        fraction /= 10;
    }
    let magnitude = integer
        .checked_mul(multiplier)
        .and_then(|value| value.checked_add(fraction + i128::from(remainder >= 5)))
        .ok_or_else(overflow)?;
    i64::try_from(if negative { -magnitude } else { magnitude }).map_err(|_| overflow())
}

fn exponent(bytes: &[u8], cursor: &mut usize) -> Result<i64, QueryError> {
    if !matches!(bytes.get(*cursor), Some(b'e' | b'E')) {
        return Ok(0);
    }
    let marker = *cursor;
    let mut end = marker + 1;
    while bytes.get(end).is_some_and(|byte| whitespace(*byte)) {
        end += 1;
    }
    let negative = bytes.get(end) == Some(&b'-');
    if matches!(bytes.get(end), Some(b'+' | b'-')) {
        end += 1;
    }
    let start = end;
    let mut value = 0_i64;
    while let Some(digit) = bytes.get(end).filter(|byte| byte.is_ascii_digit()) {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(digit - b'0'));
        end += 1;
    }
    if end == start {
        return Ok(0);
    }
    if bytes[marker + 1..start]
        .iter()
        .any(|byte| whitespace(*byte))
    {
        return Err(CassieError::SizeInput(SizeInputError::InvalidNumeric(
            std::str::from_utf8(&bytes[..end])
                .unwrap_or("")
                .trim_start()
                .to_owned(),
        ))
        .into());
    }
    if value > i64::from(i32::MAX / 2) {
        return Err(numeric_overflow());
    }
    *cursor = end;
    Ok(if negative { -value } else { value })
}
