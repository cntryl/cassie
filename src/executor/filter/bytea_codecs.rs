//! PostgreSQL 18 encode.c codec semantics, including its permissive Base64 padding.
use super::{invalid, QueryError, Work};

pub(super) fn hex_bytes(input: &str, work: &Work<'_>) -> Result<Vec<u8>, QueryError> {
    let mut output = Vec::with_capacity(input.len() / 2);
    crate::types::bytea_input::scan_hex::<crate::app::CassieError>(
        input,
        || work.check().map_err(Into::into),
        |byte| output.push(byte),
    )?;
    Ok(output)
}

pub(super) fn decode(input: &str, format: &str, work: &Work<'_>) -> Result<Vec<u8>, QueryError> {
    if format.eq_ignore_ascii_case("hex") {
        return hex_bytes(input, work);
    }
    if format.eq_ignore_ascii_case("base64") {
        return base64_decode(input, work);
    }
    if !format.eq_ignore_ascii_case("escape") {
        return Err(encoding_error(format));
    }
    let mut output = Vec::with_capacity(input.len());
    crate::types::bytea_input::scan_escape::<crate::app::CassieError>(
        input,
        || work.check().map_err(Into::into),
        |byte| output.push(byte),
    )?;
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
        let mut output = String::with_capacity(bytes.len() * 2);
        crate::types::bytea_input::append_hex(&mut output, bytes, || work.check())?;
        return Ok(output);
    }
    if format.eq_ignore_ascii_case("escape") {
        let mut output = String::with_capacity(bytes.len() * 4);
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
        return Err(encoding_error(format));
    }
    let mut output = String::with_capacity(
        super::work::base64_capacity(bytes.len()).expect("admitted Base64 capacity"),
    );
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

fn encoding_error(format: &str) -> QueryError {
    let mut message = String::with_capacity(
        super::work::encoding_error_capacity(format).expect("admitted encoding diagnostic"),
    );
    message.push_str(super::work::ENCODING_ERROR_PREFIX);
    message.push_str(format);
    message.push('"');
    invalid(message)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, Work};
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use crate::types::Value;
    #[test]
    fn should_interrupt_codec_given_cancellation_after_admission_when_bytes_are_consumed() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        let work = Work::admit("decode", &[], Some(&controls)).unwrap();
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

    #[test]
    fn should_bound_encoding_allocations_given_padding_and_line_boundaries_when_formats_encode() {
        // Arrange
        for length in [0, 1, 2, 3, 56, 57, 58] {
            let bytes = vec![255; length];
            let carrier = Value::String(format!("\\x{}", "ff".repeat(length)));
            for format in ["hex", "escape", "base64"] {
                let controls = QueryExecutionControls::from_limits(
                    &crate::config::CassieRuntimeLimits::default(),
                    std::time::Instant::now(),
                );
                let args = [carrier.clone(), Value::String(format.into())];
                let work = Work::admit("encode", &args, Some(&controls)).unwrap();
                // Act
                let encoded = encode(&bytes, format, &work).unwrap();
                // Assert
                assert!(
                    bytes.capacity() + encoded.capacity() <= controls.peak_query_memory_bytes()
                );
                if length == 57 && format == "base64" {
                    assert_eq!(encoded, format!("{}\n", "/".repeat(76)));
                }
                drop(work);
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }

    #[test]
    fn should_bound_decoding_allocations_given_padding_and_line_boundaries_when_formats_round_trip()
    {
        // Arrange
        for length in [0, 1, 2, 3, 56, 57, 58] {
            let bytes = vec![255; length];
            let carrier = Value::String(format!("\\x{}", "ff".repeat(length)));
            for format in ["hex", "escape", "base64"] {
                let args = [carrier.clone(), Value::String(format.into())];
                let encoding_work = Work::admit("encode", &args, None).unwrap();
                let encoded = encode(&bytes, format, &encoding_work).unwrap();
                let controls = QueryExecutionControls::from_limits(
                    &crate::config::CassieRuntimeLimits::default(),
                    std::time::Instant::now(),
                );
                let args = [Value::String(encoded.clone()), Value::String(format.into())];
                let work = Work::admit("decode", &args, Some(&controls)).unwrap();
                let admitted = controls.current_query_memory_bytes();
                // Act
                let decoded = decode(&encoded, format, &work).unwrap();
                // Assert
                assert_eq!(decoded, bytes, "{format} length {length}");
                assert!(decoded.capacity() + 2 * decoded.len() + 2 <= admitted);
                drop(work);
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }
}
