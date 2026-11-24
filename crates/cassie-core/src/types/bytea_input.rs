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
    scan(input, || Ok(()), |_| {})
}

/// Validate before allocating the canonical lower-case hex carrier.
pub(crate) fn canonical(input: &str) -> Result<String, ByteaInputError> {
    let mut length = 0_usize;
    scan(input, || Ok::<_, ByteaInputError>(()), |_| length += 1)?;
    let mut output = String::with_capacity(length.saturating_mul(2).saturating_add(2));
    output.push_str("\\x");
    scan(
        input,
        || Ok::<_, ByteaInputError>(()),
        |byte| push_hex_byte(&mut output, byte),
    )?;
    Ok(output)
}

fn scan<E: From<ByteaInputError>>(
    input: &str,
    check: impl FnMut() -> Result<(), E>,
    emit: impl FnMut(u8),
) -> Result<(), E> {
    if let Some(digits) = input.strip_prefix("\\x") {
        return scan_hex(digits, check, emit);
    }
    scan_escape(input, check, emit)
}

pub(crate) fn scan_escape<E: From<ByteaInputError>>(
    input: &str,
    mut check: impl FnMut() -> Result<(), E>,
    mut emit: impl FnMut(u8),
) -> Result<(), E> {
    let mut bytes = input.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        check()?;
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
            return Err(ByteaInputError::EscapeSyntax.into());
        }
        emit(((first - b'0') << 6) | ((second - b'0') << 3) | (third - b'0'));
    }
    Ok(())
}

pub(crate) fn scan_hex<E: From<ByteaInputError>>(
    input: &str,
    mut check: impl FnMut() -> Result<(), E>,
    mut emit: impl FnMut(u8),
) -> Result<(), E> {
    let mut bytes = input.chars();
    while let Some(first) = bytes.next() {
        check()?;
        if matches!(first, ' ' | '\t' | '\n' | '\r') {
            continue;
        }
        let high = nibble(first)?;
        let low = nibble(bytes.next().ok_or(ByteaInputError::HexOdd)?)?;
        emit((high << 4) | low);
    }
    Ok(())
}

pub(crate) fn nibble(character: char) -> Result<u8, ByteaInputError> {
    character
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
        .ok_or(ByteaInputError::HexDigit(character))
}

fn push_hex_byte(output: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push(char::from(HEX[usize::from(byte >> 4)]));
    output.push(char::from(HEX[usize::from(byte & 15)]));
}

/// Append canonical digits while allowing callers to enforce interruption.
pub(crate) fn append_hex<E>(
    output: &mut String,
    bytes: &[u8],
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    for (index, byte) in bytes.iter().enumerate() {
        if index.is_multiple_of(4096) {
            check()?;
        }
        push_hex_byte(output, *byte);
    }
    Ok(())
}
