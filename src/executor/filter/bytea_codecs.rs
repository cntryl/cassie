//! PostgreSQL 18 encode.c codec semantics, including its permissive Base64 padding.
use super::{invalid, QueryError, Work};

pub(super) fn hex_bytes(input: &str, work: &Work<'_>) -> Result<Vec<u8>, QueryError> {
    let mut output = Vec::with_capacity(input.len() / 2);
    let mut chars = input.chars();
    while let Some(high) = chars.next() {
        work.check()?;
        if matches!(high, ' ' | '\t' | '\n' | '\r') {
            continue;
        }
        let high = digit(high)?;
        let low = digit(
            chars
                .next()
                .ok_or_else(|| bytea(crate::types::ByteaInputError::HexOdd))?,
        )?;
        output.push(high * 16 + low);
    }
    Ok(output)
}
fn digit(character: char) -> Result<u8, QueryError> {
    character
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
        .ok_or_else(|| bytea(crate::types::ByteaInputError::HexDigit(character)))
}
fn bytea(error: crate::types::ByteaInputError) -> QueryError {
    QueryError::Cassie(error.into())
}

pub(super) fn decode(input: &str, format: &str, work: &Work<'_>) -> Result<Vec<u8>, QueryError> {
    if format.eq_ignore_ascii_case("hex") {
        return hex_bytes(input, work);
    }
    if format.eq_ignore_ascii_case("base64") {
        return base64_decode(input, work);
    }
    if !format.eq_ignore_ascii_case("escape") {
        return Err(invalid(format!("unrecognized encoding: \"{format}\"")));
    }
    let mut output = Vec::with_capacity(input.len());
    let mut bytes = input.bytes();
    while let Some(byte) = bytes.next() {
        work.check()?;
        if byte != b'\\' {
            output.push(byte);
            continue;
        }
        let first = bytes
            .next()
            .ok_or_else(|| bytea(crate::types::ByteaInputError::EscapeSyntax))?;
        if first == b'\\' {
            output.push(first);
            continue;
        }
        let second = bytes
            .next()
            .ok_or_else(|| bytea(crate::types::ByteaInputError::EscapeSyntax))?;
        let third = bytes
            .next()
            .ok_or_else(|| bytea(crate::types::ByteaInputError::EscapeSyntax))?;
        if !matches!(first, b'0'..=b'3')
            || !matches!(second, b'0'..=b'7')
            || !matches!(third, b'0'..=b'7')
        {
            return Err(bytea(crate::types::ByteaInputError::EscapeSyntax));
        }
        output.push((first - b'0') * 64 + (second - b'0') * 8 + (third - b'0'));
    }
    Ok(output)
}
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn base64_decode(input: &str, work: &Work<'_>) -> Result<Vec<u8>, QueryError> {
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let mut position = 0;
    let mut padding = 0;
    let mut bits = 0_u32;
    for character in input.chars() {
        work.check()?;
        if matches!(character, ' ' | '\t' | '\n' | '\r') {
            continue;
        }
        let sextet = if character == '=' {
            if padding == 0 {
                padding = match position {
                    2 => 1,
                    3 => 2,
                    _ => {
                        return Err(invalid(
                            "unexpected \"=\" while decoding base64 sequence".into(),
                        ))
                    }
                };
            }
            0
        } else {
            ALPHABET
                .iter()
                .position(|byte| char::from(*byte) == character)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    invalid(format!(
                        "invalid symbol \"{character}\" found while decoding base64 sequence"
                    ))
                })?
        };
        bits = (bits << 6) | sextet;
        position += 1;
        if position == 4 {
            let bytes = bits.to_be_bytes();
            output.push(bytes[1]);
            if padding == 0 || padding > 1 {
                output.push(bytes[2]);
            }
            if padding == 0 || padding > 2 {
                output.push(bytes[3]);
            }
            position = 0;
            bits = 0;
        }
    }
    if position != 0 {
        return Err(invalid("invalid base64 end sequence".into()));
    }
    Ok(output)
}

pub(super) fn encode(bytes: &[u8], format: &str, work: &Work<'_>) -> Result<String, QueryError> {
    use std::fmt::Write;
    if format.eq_ignore_ascii_case("hex") {
        return Ok(super::carrier(bytes, work)?[2..].to_owned());
    }
    if format.eq_ignore_ascii_case("escape") {
        let mut output = String::new();
        for byte in bytes {
            work.check()?;
            if *byte == b'\\' {
                output.push_str("\\\\");
            } else if *byte == 0 || *byte >= 128 {
                write!(output, "\\{byte:03o}").expect("String write");
            } else {
                output.push(char::from(*byte));
            }
        }
        return Ok(output);
    }
    if !format.eq_ignore_ascii_case("base64") {
        return Err(invalid(format!("unrecognized encoding: \"{format}\"")));
    }
    let mut output = String::new();
    let mut line = 0;
    for chunk in bytes.chunks(3) {
        work.check()?;
        let bits = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for offset in 0..4 {
            output.push(if offset > chunk.len() {
                '='
            } else {
                char::from(
                    ALPHABET[usize::try_from((bits >> (18 - 6 * offset)) & 63).expect("sextet")],
                )
            });
        }
        if chunk.len() == 3 {
            line += 4;
            if line == 76 {
                output.push('\n');
                line = 0;
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{decode, Work};
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    #[test]
    fn should_interrupt_codec_given_cancellation_after_admission_when_bytes_are_consumed() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        let work = Work::admit(&[], Some(&controls)).unwrap();
        cancellation.cancel();
        // Act
        let result = decode("00ff", "hex", &work).map_err(crate::app::CassieError::from);
        // Assert
        assert!(matches!(
            result,
            Err(crate::app::CassieError::QueryCancelled)
        ));
        drop(work);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
