//! Validate text parameters before invoking a type's input parser.
use super::ExtendedQueryError;

pub(super) fn decode(bytes: &[u8]) -> Result<&str, ExtendedQueryError> {
    let decoded = std::str::from_utf8(bytes);
    let invalid = decoded.as_ref().err().map(std::str::Utf8Error::valid_up_to);
    let nul = bytes.iter().position(|byte| *byte == 0);
    let first = match (invalid, nul) {
        (Some(invalid), Some(nul)) => Some(invalid.min(nul)),
        (invalid, nul) => invalid.or(nul),
    };
    if let Some(position) = first {
        return Err(error_at(bytes, position));
    }
    decoded.map_err(|error| error_at(bytes, error.valid_up_to()))
}

fn error_at(bytes: &[u8], position: usize) -> ExtendedQueryError {
    let suffix = &bytes[position..];
    // PostgreSQL reports up to one encoded character (at most four bytes),
    // even when the character is malformed or truncated.
    let width = match suffix[0] {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    };
    ExtendedQueryError::invalid_character_encoding(&suffix[..width.min(suffix.len())])
}
