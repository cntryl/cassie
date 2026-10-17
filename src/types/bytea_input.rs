//! PostgreSQL BYTEA text input, independent of its canonical hex storage carrier.
use std::fmt::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteaInputError {
    /// A backslash must introduce another backslash or exactly three octal digits.
    EscapeSyntax,
    /// A hexadecimal byte is missing its second digit.
    HexOdd,
    /// The first invalid character in a hexadecimal pair.
    HexDigit(char),
}

impl ByteaInputError {
    pub(crate) const fn sql_state(self) -> &'static str {
        match self {
            Self::EscapeSyntax => "22P02",
            Self::HexOdd | Self::HexDigit(_) => "22023",
        }
    }
}

impl fmt::Display for ByteaInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EscapeSyntax => formatter.write_str("invalid input syntax for type bytea"),
            Self::HexOdd => formatter.write_str("invalid hexadecimal data: odd number of digits"),
            Self::HexDigit(byte) => {
                formatter.write_str("invalid hexadecimal digit: \"")?;
                formatter.write_char(*byte)?;
                formatter.write_char('"')
            }
        }
    }
}

impl std::error::Error for ByteaInputError {}

/// Validate without allocating a decoded buffer or copying the input.
pub(crate) fn validate(input: &str) -> Result<(), ByteaInputError> {
    scan(input, |_| {})
}

/// Validate before allocating the canonical lower-case hex carrier.
pub(crate) fn canonical(input: &str) -> Result<String, ByteaInputError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut length = 0_usize;
    scan(input, |_| length += 1)?;
    let mut output = String::with_capacity(length.saturating_mul(2).saturating_add(2));
    output.push_str("\\x");
    scan(input, |byte| {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    })?;
    Ok(output)
}

fn scan(input: &str, mut emit: impl FnMut(u8)) -> Result<(), ByteaInputError> {
    if let Some(digits) = input.strip_prefix("\\x") {
        return hex(digits, emit);
    }
    let mut bytes = input.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            emit(byte);
            continue;
        }
        let first = bytes.next().ok_or(ByteaInputError::EscapeSyntax)?;
        if first == b'\\' {
            emit(first);
            continue;
        }
        let second = bytes.next().ok_or(ByteaInputError::EscapeSyntax)?;
        let third = bytes.next().ok_or(ByteaInputError::EscapeSyntax)?;
        if !matches!(first, b'0'..=b'3')
            || !matches!(second, b'0'..=b'7')
            || !matches!(third, b'0'..=b'7')
        {
            return Err(ByteaInputError::EscapeSyntax);
        }
        emit(((first - b'0') << 6) | ((second - b'0') << 3) | (third - b'0'));
    }
    Ok(())
}

fn hex(input: &str, mut emit: impl FnMut(u8)) -> Result<(), ByteaInputError> {
    let mut bytes = input.chars();
    while let Some(first) = bytes.next() {
        if matches!(first, ' ' | '\t' | '\n' | '\r') {
            continue;
        }
        let high = nibble(first)?;
        let low = nibble(bytes.next().ok_or(ByteaInputError::HexOdd)?)?;
        emit((high << 4) | low);
    }
    Ok(())
}

fn nibble(character: char) -> Result<u8, ByteaInputError> {
    character
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
        .ok_or(ByteaInputError::HexDigit(character))
}
