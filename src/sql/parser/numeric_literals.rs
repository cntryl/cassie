//! PostgreSQL radix and digit-separator syntax over existing scalar carriers.

use std::borrow::Cow;

use super::{Expr, SqlError};

pub(super) fn parse(raw: &str) -> Result<Option<Expr>, SqlError> {
    let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    if !unsigned.starts_with(|character: char| character.is_ascii_digit() || character == '.') {
        return Ok(None);
    }
    if unsigned.starts_with('.') && !unsigned.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
        return Ok(None);
    }
    for (prefixes, radix) in [(["0x", "0X"], 16), (["0o", "0O"], 8), (["0b", "0B"], 2)] {
        if prefixes.iter().any(|prefix| unsigned.starts_with(prefix)) {
            return parse_radix(raw, &unsigned[2..], radix).map(Some);
        }
    }

    let normalized = normalize_decimal(raw)?;
    let digits = normalized.strip_prefix(['+', '-']).unwrap_or(&normalized);
    if digits.bytes().all(|byte| byte.is_ascii_digit()) {
        if digits.trim_start_matches('0').len() > 131_072 {
            return Err(SqlError::numeric_range(
                "value overflows numeric format".into(),
            ));
        }
        return normalized
            .parse::<i64>()
            .map(Expr::IntegerLiteral)
            .map(Some)
            .or_else(|_| {
                Ok(Some(Expr::NumericLiteral(format!(
                    "{}{}",
                    if normalized.starts_with('-') { "-" } else { "" },
                    digits.trim_start_matches('0')
                ))))
            });
    }
    let value = normalized
        .parse::<f64>()
        .map_err(|_| invalid_literal(raw))?;
    // PostgreSQL types a decimal or exponent literal as `numeric`; the
    // binder narrows it to `double precision` where a float peer selects it.
    let _ = value;
    crate::types::extended::numeric::parse_text(&normalized)
        .map_err(|error| SqlError::numeric_range(error.message))?;
    Ok(Some(Expr::NumericLiteral(normalized.into_owned())))
}

fn parse_radix(raw: &str, digits: &str, radix: u32) -> Result<Expr, SqlError> {
    let digits = digits.strip_prefix('_').unwrap_or(digits);
    if digits.is_empty() {
        return Err(invalid_literal(raw));
    }
    let bytes = digits.as_bytes();
    let mut value = Some(0u64);
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == b'_' {
            if index == 0
                || !bytes[index - 1].is_ascii_hexdigit()
                || !bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            {
                return Err(invalid_literal(raw));
            }
            continue;
        }
        let digit = char::from(byte)
            .to_digit(radix)
            .ok_or_else(|| invalid_literal(raw))?;
        value = value.and_then(|value| {
            value
                .checked_mul(u64::from(radix))
                .and_then(|value| value.checked_add(u64::from(digit)))
        });
    }
    let value = value.ok_or_else(|| unsupported_numeric(raw))?;
    let signed = if raw.starts_with('-') {
        i64::try_from(value)
            .ok()
            .and_then(i64::checked_neg)
            .or_else(|| (value == 1u64 << 63).then_some(i64::MIN))
    } else {
        i64::try_from(value).ok()
    };
    Ok(signed.map_or_else(
        || {
            Expr::NumericLiteral(format!(
                "{}{value}",
                if raw.starts_with('-') { "-" } else { "" }
            ))
        },
        Expr::IntegerLiteral,
    ))
}

fn normalize_decimal(raw: &str) -> Result<Cow<'_, str>, SqlError> {
    let bytes = raw.as_bytes();
    if !bytes.contains(&b'_') {
        return Ok(Cow::Borrowed(raw));
    }
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'_'
            && (index == 0
                || !bytes[index - 1].is_ascii_digit()
                || !bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
        {
            return Err(invalid_literal(raw));
        }
    }
    Ok(Cow::Owned(raw.replace('_', "")))
}

pub(in crate::sql::parser) fn is_numeric_mantissa(raw: &str) -> bool {
    !raw.is_empty() && normalize_decimal(raw).is_ok_and(|value| value.parse::<f64>().is_ok())
}

fn invalid_literal(raw: &str) -> SqlError {
    SqlError::syntax(format!("invalid numeric literal '{raw}'"))
}

fn unsupported_numeric(raw: &str) -> SqlError {
    SqlError::unsupported(format!(
        "numeric literal '{raw}' requires unsupported NUMERIC literal storage"
    ))
}
