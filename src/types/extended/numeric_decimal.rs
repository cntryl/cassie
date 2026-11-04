//! Borrowed decimal format checks shared by numeric input and JSONB admission.
use super::{overflow, TypeInputError, MAX_DSCALE, MAX_INTEGER_DIGITS};

#[derive(Clone, Copy)]
pub(crate) struct Shape {
    pub(crate) scale: i64,
    pub(crate) significant_digits: usize,
    pub(crate) integer_digits: usize,
}

/// The caller has already validated its SQL or JSON coefficient grammar.
/// Counting the first nonzero digit does not construct a coefficient or power.
pub(crate) fn shape(integer: &str, fraction: &str, exponent: i64) -> Result<Shape, TypeInputError> {
    if exponent.unsigned_abs() > i64::from(i32::MAX / 2).unsigned_abs() {
        return Err(overflow());
    }
    let scale = i64::try_from(fraction.len())
        .map_err(|_| overflow())?
        .checked_sub(exponent)
        .ok_or_else(overflow)?;
    if scale > i64::from(MAX_DSCALE) {
        return Err(overflow());
    }
    let significant_digits = integer
        .bytes()
        .chain(fraction.bytes())
        .skip_while(|digit| *digit == b'0')
        .count();
    let integer_digits = if significant_digits == 0 {
        0
    } else {
        usize::try_from(
            i64::try_from(significant_digits)
                .map_err(|_| overflow())?
                .checked_sub(scale)
                .ok_or_else(overflow)?
                .max(0),
        )
        .map_err(|_| overflow())?
    };
    if integer_digits > MAX_INTEGER_DIGITS {
        return Err(overflow());
    }
    Ok(Shape {
        scale,
        significant_digits,
        integer_digits,
    })
}

pub(crate) struct JsonParts<'a> {
    pub(crate) integer: &'a str,
    pub(crate) fraction: &'a str,
    pub(crate) negative: bool,
    pub(crate) shape: Shape,
}

impl JsonParts<'_> {
    pub(crate) fn text_bytes(&self) -> Result<usize, TypeInputError> {
        let scale = usize::try_from(self.shape.scale.max(0)).map_err(|_| overflow())?;
        self.shape
            .integer_digits
            .max(1)
            .checked_add(scale)
            .and_then(|bytes| bytes.checked_add(usize::from(scale > 0)))
            .and_then(|bytes| {
                bytes.checked_add(usize::from(
                    self.negative && self.shape.significant_digits > 0,
                ))
            })
            .ok_or_else(overflow)
    }
}

/// Strict local JSON number grammar; this does not parse a whole document.
pub(crate) fn json_parts(text: &str) -> Result<JsonParts<'_>, TypeInputError> {
    let negative = text.starts_with('-');
    let body = text.strip_prefix('-').unwrap_or(text);
    let (coefficient, raw_exponent) = body.find(['e', 'E']).map_or((body, None), |index| {
        (&body[..index], Some(&body[index + 1..]))
    });
    let (integer, fraction) = coefficient.split_once('.').unwrap_or((coefficient, ""));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(integer)
        || (integer.len() > 1 && integer.starts_with('0'))
        || (coefficient.contains('.') && !digits(fraction))
    {
        return Err(invalid_json_number());
    }
    let exponent = if let Some(raw) = raw_exponent {
        let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
        if !digits(unsigned) {
            return Err(invalid_json_number());
        }
        raw.parse::<i64>().unwrap_or(i64::MAX)
    } else {
        0
    };
    Ok(JsonParts {
        integer,
        fraction,
        negative,
        shape: shape(integer, fraction, exponent)?,
    })
}

fn invalid_json_number() -> TypeInputError {
    TypeInputError::new(
        "22P02",
        "invalid input syntax for type json: invalid numeric token".into(),
    )
}
