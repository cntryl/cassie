//! Borrowed decimal FLOAT8 input and native non-finite spellings.
/// Semantic input errors for the existing DOUBLE PRECISION identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FloatInputError {
    #[error("invalid input syntax for type double precision")]
    Syntax,
    #[error("double precision out of range")]
    Range,
}

impl FloatInputError {
    /// Returns the PostgreSQL SQLSTATE for this input error.
    #[must_use]
    pub const fn sql_state(self) -> &'static str {
        match self {
            Self::Syntax => "22P02",
            Self::Range => "22003",
        }
    }
}

/// Parses PostgreSQL FLOAT8 text input.
///
/// # Errors
///
/// Returns an error when the input has invalid syntax or is outside FLOAT8 range.
pub fn parse(input: &str) -> Result<f64, FloatInputError> {
    let input = input.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\u{b}');
    let magnitude = input.strip_prefix(['+', '-']).unwrap_or(input);
    if magnitude.eq_ignore_ascii_case("nan") {
        return Ok(f64::NAN);
    }
    if magnitude.eq_ignore_ascii_case("inf") || magnitude.eq_ignore_ascii_case("infinity") {
        return Ok(if input.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    let value = input.parse::<f64>().map_err(|_| FloatInputError::Syntax)?;
    let nonzero = input
        .split(['e', 'E'])
        .next()
        .is_some_and(|mantissa| mantissa.bytes().any(|digit| (b'1'..=b'9').contains(&digit)));
    if !value.is_finite() || (value == 0.0 && nonzero) {
        return Err(FloatInputError::Range);
    }
    Ok(value)
}
