//! Identifier lists, routine/operator signatures and SQL type names as the
//! `reg*` input functions parse them.
use crate::catalog::builtin_identity::builtin_identities;
use crate::types::ObjectIdentifierError;

/// One identifier: unquoted input is folded to lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Identifier {
    pub(super) value: String,
    pub(super) quoted: bool,
}

/// Splits `a."B".c` into identifiers (`stringToQualifiedNameList`).
pub(super) fn qualified_name(text: &str) -> Result<Vec<Identifier>, ObjectIdentifierError> {
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

/// Splits `name(arg, arg)` into the name and its argument texts.
pub(super) fn signature(text: &str) -> Result<(String, Vec<String>), ObjectIdentifierError> {
    let trimmed = text.trim();
    let Some(open) = find_unquoted(trimmed, '(') else {
        return Err(ObjectIdentifierError::Signature(
            "expected a left parenthesis".to_string(),
        ));
    };
    let Some(arguments) = trimmed[open + 1..].trim_end().strip_suffix(')') else {
        return Err(ObjectIdentifierError::Signature(
            "expected a right parenthesis".to_string(),
        ));
    };
    let name = trimmed[..open].trim().to_string();
    let arguments = if arguments.trim().is_empty() {
        Vec::new()
    } else {
        split_arguments(arguments)
    };
    Ok((name, arguments))
}

fn find_unquoted(text: &str, target: char) -> Option<usize> {
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            character if character == target && !quoted => return Some(index),
            _ => {}
        }
    }
    None
}

fn split_arguments(text: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut depth = 0_u32;
    let mut quoted = false;
    for character in text.chars() {
        match character {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                arguments.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    arguments.push(current.trim().to_string());
    arguments
}

/// The built-in type a SQL type name denotes (`parseTypeString`), resolving
/// the SQL-standard spellings, typmods and array suffixes.
pub(super) fn builtin_type_oid(text: &str) -> Result<Option<u32>, ObjectIdentifierError> {
    let mut base = text.trim();
    let mut array_depth = 0_u32;
    while let Some(stripped) = base.strip_suffix("[]") {
        base = stripped.trim_end();
        array_depth += 1;
    }
    let base = strip_typmod(base);
    let canonical = standard_type_name(&base);
    let name = if let Some(name) = canonical {
        name.to_string()
    } else {
        let parts = qualified_name(&base)?;
        match parts.as_slice() {
            [name] => name.value.clone(),
            [schema, name] if schema.value == "pg_catalog" => name.value.clone(),
            _ => return Ok(None),
        }
    };
    let identities = builtin_identities();
    let Some(mut row) = identities.type_by_name(&name) else {
        return Ok(None);
    };
    for _ in 0..array_depth {
        let Some(array) = identities.type_by_oid(row.array) else {
            return Ok(None);
        };
        row = array;
    }
    Ok(Some(row.oid))
}

/// Drops a type modifier such as `(10)` or `(3)`, keeping any trailing words
/// (`timestamp(3) with time zone` becomes `timestamp with time zone`).
fn strip_typmod(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('"') {
        return trimmed.to_string();
    }
    let Some(open) = trimmed.find('(') else {
        return trimmed.to_string();
    };
    let Some(close) = trimmed[open..].find(')').map(|offset| open + offset) else {
        return trimmed.to_string();
    };
    let head = trimmed[..open].trim_end();
    let tail = trimmed[close + 1..].trim();
    if tail.is_empty() {
        head.to_string()
    } else {
        format!("{head} {tail}")
    }
}

fn standard_type_name(text: &str) -> Option<&'static str> {
    if text.starts_with('"') {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let words = lower.split_whitespace().collect::<Vec<_>>();
    let name = match words.as_slice() {
        ["int" | "integer"] => "int4",
        ["smallint"] => "int2",
        ["bigint"] => "int8",
        ["real"] => "float4",
        ["float"] | ["double", "precision"] => "float8",
        ["decimal" | "dec" | "numeric"] => "numeric",
        ["boolean"] => "bool",
        ["character" | "char" | "nchar", "varying"]
        | ["national", "character" | "char", "varying"] => "varchar",
        ["character" | "char" | "nchar"] | ["national", "character" | "char"] => "bpchar",
        ["bit", "varying"] => "varbit",
        ["timestamp", "with", "time", "zone"] => "timestamptz",
        ["timestamp", "without", "time", "zone"] => "timestamp",
        ["time", "with", "time", "zone"] => "timetz",
        ["time", "without", "time", "zone"] => "time",
        ["interval", ..] => "interval",
        _ => return None,
    };
    Some(name)
}

/// `quote_ident`: quote unless the name is a lower-case identifier that is
/// not a reserved, column-name or type/function-name keyword.
#[must_use]
pub(crate) fn quote_identifier(value: &str) -> String {
    let simple = value
        .chars()
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_lowercase())
        && value.chars().all(|character| {
            character == '_' || character.is_ascii_lowercase() || character.is_ascii_digit()
        });
    if simple && !builtin_identities().quoted_keywords.contains(value) {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('"', "\"\""))
    }
}

#[cfg(test)]
mod tests {
    use super::{builtin_type_oid, qualified_name, quote_identifier, signature, Identifier};

    #[test]
    fn should_fold_unquoted_identifiers_and_keep_quoted_case() {
        // Arrange
        let text = r#"Public."Mixed""#;

        // Act
        let parts = qualified_name(text);

        // Assert
        assert_eq!(
            parts,
            Ok(vec![
                Identifier {
                    value: "public".to_string(),
                    quoted: false
                },
                Identifier {
                    value: "Mixed".to_string(),
                    quoted: true
                },
            ])
        );
    }

    #[test]
    fn should_reject_whitespace_inside_an_unquoted_name() {
        // Arrange
        let text = "x y";

        // Act
        let parts = qualified_name(text);

        // Assert
        assert!(parts.is_err());
    }

    #[test]
    fn should_split_a_routine_signature() {
        // Arrange
        let text = "pg_catalog.abs( integer )";

        // Act
        let parsed = signature(text);

        // Assert
        assert_eq!(
            parsed,
            Ok(("pg_catalog.abs".to_string(), vec!["integer".to_string()]))
        );
    }

    #[test]
    fn should_resolve_standard_type_spellings() {
        // Arrange
        let spellings = [
            "integer",
            "pg_catalog.int4",
            "double precision",
            "character varying(10)",
            "integer[]",
            "timestamp(3) with time zone",
        ];

        // Act
        let oids = spellings.map(|spelling| builtin_type_oid(spelling).ok().flatten());

        // Assert
        assert_eq!(
            oids,
            [
                Some(23),
                Some(23),
                Some(701),
                Some(1043),
                Some(1007),
                Some(1184)
            ]
        );
    }

    #[test]
    fn should_quote_keywords_and_mixed_case_names() {
        // Arrange
        let names = ["item", "default", "Mixed", "with space"];

        // Act
        let quoted = names.map(quote_identifier);

        // Assert
        assert_eq!(
            quoted,
            ["item", "\"default\"", "\"Mixed\"", "\"with space\""]
        );
    }
}
