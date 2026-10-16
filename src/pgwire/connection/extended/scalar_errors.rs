//! Semantic scalar-input errors, separate from malformed protocol framing.
use super::{ExtendedQueryError, PgWireError, PgWireSeverity};
use std::fmt::Write;

impl ExtendedQueryError {
    pub(super) fn invalid_input_syntax(type_name: &str, input: &str) -> Self {
        Self {
            pg_error: Box::new(PgWireError::new(
                PgWireSeverity::Error,
                "22P02",
                format!("invalid input syntax for type {type_name}: \"{input}\""),
            )),
            record_protocol_error: false,
        }
    }

    pub(super) fn numeric_value_out_of_range(type_name: &str, input: &str) -> Self {
        Self {
            pg_error: Box::new(PgWireError::new(
                PgWireSeverity::Error,
                "22003",
                format!("value \"{input}\" is out of range for type {type_name}"),
            )),
            record_protocol_error: false,
        }
    }

    pub(super) fn invalid_character_encoding(bytes: &[u8]) -> Self {
        let mut message = String::with_capacity(64);
        message.push_str("invalid byte sequence for encoding \"UTF8\": ");
        for (index, byte) in bytes.iter().take(4).enumerate() {
            if index != 0 {
                message.push(' ');
            }
            write!(&mut message, "0x{byte:02x}").expect("String formatting cannot fail");
        }
        Self {
            pg_error: Box::new(PgWireError::new(PgWireSeverity::Error, "22021", message)),
            record_protocol_error: false,
        }
    }
}
