//! `pg_lsn`: a 64-bit write-ahead log position.
use super::TypeInputError;

/// `pg_lsn_in`: two hexadecimal words of one to eight digits.
///
/// # Errors
/// Returns `22P02` for malformed text.
pub fn parse_text(text: &str) -> Result<u64, TypeInputError> {
    let error = || {
        TypeInputError::new(
            "22P02",
            format!("invalid input syntax for type pg_lsn: \"{text}\""),
        )
    };
    let (high, low) = text.split_once('/').ok_or_else(error)?;
    let word = |digits: &str| {
        if (1..=8).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            u32::from_str_radix(digits, 16).ok()
        } else {
            None
        }
    };
    let (high, low) = word(high).zip(word(low)).ok_or_else(error)?;
    Ok((u64::from(high) << 32) | u64::from(low))
}

/// `pg_lsn_out`: upper-case hexadecimal words without leading zeros.
#[must_use]
pub fn format_text(value: u64) -> String {
    format!("{:X}/{:X}", value >> 32, value & u64::from(u32::MAX))
}

/// `pg_lsn_recv`.
///
/// # Errors
/// Returns PostgreSQL's receive error for a frame that is not eight bytes.
pub fn receive_binary(bytes: &[u8]) -> Result<u64, String> {
    if bytes.len() < 8 {
        return Err(super::network::INSUFFICIENT_DATA.to_string());
    }
    <[u8; 8]>::try_from(bytes)
        .map(u64::from_be_bytes)
        .map_err(|_| "incorrect binary data format".to_string())
}

fn out_of_range() -> TypeInputError {
    TypeInputError::new("22023", "pg_lsn out of range".to_string())
}

/// `pg_lsn ± numeric` with a whole-byte offset.
///
/// # Errors
/// Returns `22023` when the result leaves the 64-bit range and `0A000` for
/// a NaN offset; fractional offsets round half away from zero.
pub fn offset(value: u64, offset: f64) -> Result<u64, TypeInputError> {
    if !offset.is_finite() {
        return Err(TypeInputError::new(
            "0A000",
            "cannot add NaN to pg_lsn".to_string(),
        ));
    }
    let offset = format!("{:.0}", offset.round())
        .parse::<i128>()
        .map_err(|_| out_of_range())?;
    let result = i128::from(value)
        .checked_add(offset)
        .ok_or_else(out_of_range)?;
    u64::try_from(result).map_err(|_| out_of_range())
}

/// `pg_lsn(numeric)`.
///
/// # Errors
/// Returns `22023` outside the 64-bit range.
pub fn from_number(value: f64) -> Result<u64, TypeInputError> {
    offset(0, value)
}

#[cfg(test)]
mod tests {
    use super::{format_text, offset, parse_text};

    #[test]
    fn should_round_trip_pg_lsn_text_in_upper_case() {
        // Arrange
        let inputs = ["00000016/b374d848", "0/0", "FFFFFFFF/FFFFFFFF"];

        // Act
        let formatted = inputs.map(|input| format_text(parse_text(input).expect("valid lsn")));

        // Assert
        assert_eq!(formatted, ["16/B374D848", "0/0", "FFFFFFFF/FFFFFFFF"]);
        assert!(parse_text("not-an-lsn").is_err());
        assert!(parse_text("123456789/0").is_err());
    }

    #[test]
    fn should_reject_pg_lsn_offsets_outside_the_range() {
        // Arrange
        let maximum = u64::MAX;

        // Act
        let overflow = offset(maximum, 1.0).map_err(|error| error.sqlstate);
        let underflow = offset(0, -1.0).map_err(|error| error.sqlstate);

        // Assert
        assert_eq!((overflow, underflow), (Err("22023"), Err("22023")));
        assert_eq!(offset(8, 8.0), Ok(16));
    }
}
