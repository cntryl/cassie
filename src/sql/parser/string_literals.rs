//! Standard, escape and Unicode string literals over the existing TEXT value.

use super::super::SqlError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Standard,
    Escape,
    Unicode,
}

pub(super) fn parse(raw: &str) -> Result<Option<String>, SqlError> {
    let (mode, quote) = if raw.starts_with('\'') {
        (Mode::Standard, 0)
    } else if raw.starts_with("E'") || raw.starts_with("e'") {
        (Mode::Escape, 1)
    } else if raw.starts_with("U&'") || raw.starts_with("u&'") {
        (Mode::Unicode, 2)
    } else {
        return Ok(None);
    };
    let (mut output, mut end) = fragment(raw, quote, mode == Mode::Escape)?;
    while let Some(next) = continuation(raw, end) {
        let (bytes, next_end) = fragment(raw, next, mode == Mode::Escape)?;
        output.extend(bytes);
        end = next_end;
    }
    let tail = super::super::lexical::trim_separators(&raw[end..]);
    let text = text_from_bytes(output)?;
    if mode == Mode::Unicode {
        let escape = unicode_escape_character(tail)?;
        return decode_unicode(&text, escape).map(Some);
    }
    if !tail.is_empty() {
        return Err(SqlError::syntax(
            "unexpected text after string literal".to_owned(),
        ));
    }
    Ok(Some(text))
}

pub(in crate::sql::parser) fn unicode_identifier_prefix(
    raw: &str,
) -> Result<(String, usize), SqlError> {
    let mut cursor = 3;
    let end = loop {
        let Some(byte) = raw.as_bytes().get(cursor) else {
            return Err(SqlError::syntax(
                "unterminated quoted identifier".to_owned(),
            ));
        };
        if *byte == b'"' {
            if raw.as_bytes().get(cursor + 1) == Some(&b'"') {
                cursor += 2;
                continue;
            }
            break cursor + 1;
        }
        cursor += 1;
    };
    let body = raw[3..end - 1].replace("\"\"", "\"");
    let suffix = super::super::lexical::separator_end(raw, end);
    let (escape, consumed) =
        if let Some(keyword_end) = super::super::lexical::pattern_end(raw, suffix, "uescape") {
            let quote = super::super::lexical::separator_end(raw, keyword_end);
            if raw.as_bytes().get(quote) != Some(&b'\'') {
                return Err(SqlError::syntax(
                    "invalid Unicode escape character".to_owned(),
                ));
            }
            let literal_end = super::super::lexical::quoted_end(raw, quote)
                .ok_or_else(|| SqlError::syntax("invalid Unicode escape character".to_owned()))?;
            (
                unicode_escape_character(&raw[suffix..literal_end])?,
                literal_end,
            )
        } else {
            ('\\', end)
        };
    Ok((decode_unicode(&body, escape)?, consumed))
}

fn continuation(raw: &str, mut index: usize) -> Option<usize> {
    let mut newline = false;
    while index < raw.len() {
        let tail = &raw[index..];
        if tail.starts_with("--") {
            index += tail.find(['\n', '\r']).unwrap_or(tail.len());
        } else if let Some(character) = tail.chars().next().filter(|value| value.is_whitespace()) {
            newline |= matches!(character, '\n' | '\r');
            index += character.len_utf8();
        } else {
            break;
        }
    }
    (newline && raw[index..].starts_with('\'')).then_some(index)
}

fn fragment(raw: &str, quote: usize, escapes: bool) -> Result<(Vec<u8>, usize), SqlError> {
    let mut output = Vec::new();
    let mut index = quote + 1;
    while index < raw.len() {
        match raw.as_bytes()[index] {
            b'\'' => {
                if raw.as_bytes().get(index + 1) == Some(&b'\'') {
                    output.push(b'\'');
                    index += 2;
                } else {
                    return Ok((output, index + 1));
                }
            }
            b'\\' if escapes => {
                index += 1;
                escape_byte(raw, &mut index, &mut output)?;
            }
            _ => {
                let character = raw[index..]
                    .chars()
                    .next()
                    .expect("remaining string character");
                let mut bytes = [0; 4];
                output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
                index += character.len_utf8();
            }
        }
    }
    Err(SqlError::syntax("unterminated quoted string".to_owned()))
}

fn escape_byte(raw: &str, index: &mut usize, output: &mut Vec<u8>) -> Result<(), SqlError> {
    let Some(byte) = raw.as_bytes().get(*index).copied() else {
        return Err(SqlError::syntax("unterminated quoted string".to_owned()));
    };
    *index += 1;
    match byte {
        b'b' => output.push(8),
        b'f' => output.push(12),
        b'n' => output.push(b'\n'),
        b'r' => output.push(b'\r'),
        b't' => output.push(b'\t'),
        b'0'..=b'7' => {
            let mut value = u16::from(byte - b'0');
            for _ in 0..2 {
                let Some(next @ b'0'..=b'7') = raw.as_bytes().get(*index).copied() else {
                    break;
                };
                value = value * 8 + u16::from(next - b'0');
                *index += 1;
            }
            output.push(value.to_le_bytes()[0]);
        }
        b'x' => {
            let mut value = None;
            for _ in 0..2 {
                let Some(digit) = raw
                    .as_bytes()
                    .get(*index)
                    .and_then(|value| char::from(*value).to_digit(16))
                else {
                    break;
                };
                value = Some(value.unwrap_or(0) * 16 + digit);
                *index += 1;
            }
            output.push(value.map_or(b'x', |value: u32| value.to_le_bytes()[0]));
        }
        b'u' | b'U' => {
            let count = if byte == b'u' { 4 } else { 8 };
            let mut value = escape_hex(raw, index, count)?;
            if (0xd800..=0xdbff).contains(&value) {
                let tail = &raw[*index..];
                let count = if tail.starts_with("\\u") {
                    4
                } else if tail.starts_with("\\U") {
                    8
                } else {
                    return Err(unicode_surrogate_error());
                };
                *index += 2;
                let low = escape_hex(raw, index, count)?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err(unicode_surrogate_error());
                }
                value = 0x10000 + ((value - 0xd800) << 10) + low - 0xdc00;
            }
            let character = unicode_character(value)?;
            let mut bytes = [0; 4];
            output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
        }
        _ if byte.is_ascii() => output.push(byte),
        _ => {
            *index -= 1;
            let character = raw[*index..]
                .chars()
                .next()
                .expect("escaped Unicode character");
            let mut bytes = [0; 4];
            output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
            *index += character.len_utf8();
        }
    }
    Ok(())
}

fn escape_hex(raw: &str, index: &mut usize, count: usize) -> Result<u32, SqlError> {
    let end = index.checked_add(count).ok_or_else(escape_error)?;
    let digits = raw.get(*index..end).ok_or_else(escape_error)?;
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(escape_error());
    }
    *index = end;
    u32::from_str_radix(digits, 16).map_err(|_| escape_error())
}

fn escape_error() -> SqlError {
    SqlError::syntax("invalid Unicode escape in escape string".to_owned())
}

fn text_from_bytes(output: Vec<u8>) -> Result<String, SqlError> {
    if output.contains(&0) {
        return Err(SqlError::syntax(
            "invalid byte sequence for encoding \"UTF8\": 0x00".to_owned(),
        ));
    }
    String::from_utf8(output).map_err(|_| {
        SqlError::syntax("invalid byte sequence for encoding \"UTF8\": escape string".to_owned())
    })
}

fn unicode_escape_character(tail: &str) -> Result<char, SqlError> {
    if tail.is_empty() {
        return Ok('\\');
    }
    let end = super::super::lexical::pattern_end(tail, 0, "uescape").ok_or_else(|| {
        SqlError::syntax("unexpected text after Unicode string literal".to_owned())
    })?;
    let spec = super::super::lexical::trim_separators(&tail[end..]);
    if !spec.starts_with('\'') {
        return Err(SqlError::syntax(
            "invalid Unicode escape character".to_owned(),
        ));
    }
    let (bytes, end) = fragment(spec, 0, false)?;
    if !super::super::lexical::trim_separators(&spec[end..]).is_empty() {
        return Err(SqlError::syntax("unexpected text after UESCAPE".to_owned()));
    }
    let value = text_from_bytes(bytes)?;
    let mut chars = value.chars();
    let escape = chars
        .next()
        .ok_or_else(|| SqlError::syntax("invalid Unicode escape character".to_owned()))?;
    if chars.next().is_some()
        || escape.is_ascii_hexdigit()
        || escape.is_whitespace()
        || matches!(escape, '+' | '\'' | '"')
    {
        return Err(SqlError::syntax(
            "invalid Unicode escape character".to_owned(),
        ));
    }
    Ok(escape)
}

pub(super) fn decode_unicode(raw: &str, escape: char) -> Result<String, SqlError> {
    let mut chars = raw.chars().peekable();
    let mut output = String::new();
    let mut high: Option<u32> = None;
    while let Some(character) = chars.next() {
        if character != escape {
            if high.is_some() {
                return Err(unicode_surrogate_error());
            }
            output.push(character);
            continue;
        }
        if chars.peek() == Some(&escape) {
            let _ = chars.next();
            if high.is_some() {
                return Err(unicode_surrogate_error());
            }
            output.push(escape);
            continue;
        }
        let count = if chars.peek() == Some(&'+') {
            let _ = chars.next();
            6
        } else {
            4
        };
        let mut value = 0u32;
        for _ in 0..count {
            let digit = chars
                .next()
                .and_then(|character| character.to_digit(16))
                .ok_or_else(|| SqlError::syntax("invalid Unicode escape".to_owned()))?;
            value = value * 16 + digit;
        }
        if let Some(upper) = high.take() {
            if !(0xdc00..=0xdfff).contains(&value) {
                return Err(unicode_surrogate_error());
            }
            value = 0x1_0000u32 + ((upper - 0xd800) << 10) + value - 0xdc00;
        } else if (0xd800..=0xdbff).contains(&value) {
            high = Some(value);
            continue;
        } else if (0xdc00..=0xdfff).contains(&value) {
            return Err(unicode_surrogate_error());
        }
        output.push(unicode_character(value)?);
    }
    if high.is_some() {
        return Err(unicode_surrogate_error());
    }
    Ok(output)
}

fn unicode_character(value: u32) -> Result<char, SqlError> {
    if value == 0 {
        return Err(SqlError::syntax("invalid Unicode escape value".to_owned()));
    }
    char::from_u32(value).ok_or_else(|| SqlError::syntax("invalid Unicode escape value".to_owned()))
}

fn unicode_surrogate_error() -> SqlError {
    SqlError::syntax("invalid Unicode surrogate pair".to_owned())
}
