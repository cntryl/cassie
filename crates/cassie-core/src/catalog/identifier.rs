//! SQL identifier components and pure qualified-name tokenization.

use crate::types::ObjectIdentifierError;

/// One identifier: unquoted input is folded to lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
pub struct Identifier {
    pub value: String,
    pub quoted: bool,
}

/// Splits `a."B".c` into identifiers (`stringToQualifiedNameList`).
///
/// # Errors
/// Returns an error for malformed, empty or unclosed quoted components.
#[doc(hidden)]
pub fn qualified_name(text: &str) -> Result<Vec<Identifier>, ObjectIdentifierError> {
    let invalid = || ObjectIdentifierError::InvalidName(text.to_string());
    let mut parts = Vec::new();
    let mut chars = text.trim().chars().peekable();
    loop {
        let mut value = String::new();
        let quoted = chars.peek() == Some(&'"');
        if quoted {
            chars.next();
            loop {
                match chars.next() {
                    Some('"') if chars.peek() == Some(&'"') => {
                        chars.next();
                        value.push('"');
                    }
                    Some('"') => break,
                    Some(character) => value.push(character),
                    None => return Err(invalid()),
                }
            }
            if value.is_empty() {
                return Err(invalid());
            }
        } else {
            while let Some(&character) = chars.peek() {
                if character == '.' || character.is_whitespace() {
                    break;
                }
                if character == '"' {
                    return Err(invalid());
                }
                value.push(character.to_ascii_lowercase());
                chars.next();
            }
            if value.is_empty() {
                return Err(invalid());
            }
        }
        parts.push(Identifier { value, quoted });
        while chars
            .peek()
            .is_some_and(|character| character.is_whitespace())
        {
            chars.next();
        }
        match chars.next() {
            None => break,
            Some('.') => {
                while chars
                    .peek()
                    .is_some_and(|character| character.is_whitespace())
                {
                    chars.next();
                }
            }
            Some(_) => return Err(invalid()),
        }
    }
    Ok(parts)
}

/// Reads leading qualified-name components before a signature or typemod.
#[doc(hidden)]
pub fn leading_qualified_name(text: &str) -> Vec<Identifier> {
    let mut parts = Vec::new();
    let mut chars = text.trim().chars().peekable();
    loop {
        let mut value = String::new();
        let quoted = chars.peek() == Some(&'"');
        if quoted {
            chars.next();
            loop {
                match chars.next() {
                    Some('"') if chars.peek() == Some(&'"') => {
                        chars.next();
                        value.push('"');
                    }
                    Some('"') => break,
                    Some(character) => value.push(character),
                    None => return parts,
                }
            }
        } else {
            while let Some(&character) = chars.peek() {
                if matches!(character, '.' | '(' | '[' | '"') || character.is_whitespace() {
                    break;
                }
                value.push(character.to_ascii_lowercase());
                chars.next();
            }
        }
        if value.is_empty() {
            return parts;
        }
        parts.push(Identifier { value, quoted });
        while chars
            .peek()
            .is_some_and(|character| character.is_whitespace())
        {
            chars.next();
        }
        if chars.next() != Some('.') {
            return parts;
        }
        while chars
            .peek()
            .is_some_and(|character| character.is_whitespace())
        {
            chars.next();
        }
    }
}
