//! Label literals and statement bodies of enum DDL (#827).
//!
//! `CREATE TYPE ... AS ENUM ('a', 'b')` and `ALTER TYPE ... ADD VALUE` /
//! `RENAME VALUE` carry labels as single-quoted string literals. Label length
//! and uniqueness are checked at execution, where PostgreSQL reports them.
use super::{ParsedStatement, QueryStatement, SqlError};
use crate::sql::ast::{TypeAction, TypeStatement};
use crate::types::EnumValuePlacement;

/// Strips the keyword `enum` (case-insensitive) when it is a whole word.
pub(super) fn strip_enum_keyword(body: &str) -> Option<&str> {
    let lower = body.to_ascii_lowercase();
    let rest = lower.strip_prefix("enum")?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with('(')) {
        return None;
    }
    Some(body["enum".len()..].trim_start())
}

/// Reads one `'...'` literal from the start of `text` and returns its decoded
/// value (each doubled `''` is one quote) and the trimmed remainder.
pub(super) fn take_label(text: &str) -> Result<(String, &str), SqlError> {
    let text = text.trim_start();
    let Some(body) = text.strip_prefix('\'') else {
        return Err(SqlError::syntax(
            "an enum label must be a single-quoted string literal".into(),
        ));
    };
    let mut label = String::new();
    let mut characters = body.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character != '\'' {
            label.push(character);
            continue;
        }
        if matches!(characters.peek(), Some((_, '\''))) {
            characters.next();
            label.push('\'');
            continue;
        }
        return Ok((label, body[index + 1..].trim_start()));
    }
    Err(SqlError::syntax(
        "unterminated quoted string in an enum label".into(),
    ))
}

/// Parses the text between the parentheses of `AS ENUM (...)`.
pub(super) fn parse_label_list(inner: &str) -> Result<Vec<String>, SqlError> {
    let mut labels = Vec::new();
    let mut rest = inner.trim();
    if rest.is_empty() {
        return Ok(labels);
    }
    loop {
        let (label, after) = take_label(rest)?;
        labels.push(label);
        if after.is_empty() {
            return Ok(labels);
        }
        rest = after
            .strip_prefix(',')
            .ok_or_else(|| SqlError::syntax("enum labels must be separated by commas".into()))?;
    }
}

/// Parses what follows `CREATE TYPE name AS ENUM`.
pub(super) fn parse_create_enum(
    raw_sql: &str,
    name: String,
    after_enum: &str,
) -> Result<ParsedStatement, SqlError> {
    let inner = after_enum
        .strip_prefix('(')
        .and_then(|body| body.strip_suffix(')'))
        .ok_or_else(|| {
            SqlError::syntax("CREATE TYPE AS ENUM requires a parenthesized label list".into())
        })?;
    Ok(ParsedStatement {
        raw_sql: raw_sql.to_owned(),
        statement: QueryStatement::Type(TypeStatement {
            action: TypeAction::CreateEnum {
                name,
                labels: parse_label_list(inner)?,
            },
        }),
    })
}

/// Strips a leading keyword (case-insensitive, whole word).
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    let rest = lower.strip_prefix(word)?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    Some(text[word.len()..].trim_start())
}

/// The parsed tail of `ADD VALUE`.
#[derive(Debug)]
pub(super) struct AddValue {
    pub label: String,
    pub if_not_exists: bool,
    pub placement: Option<EnumValuePlacement>,
}

/// Parses what follows `ALTER TYPE name ADD VALUE`:
/// `[IF NOT EXISTS] 'label' [BEFORE | AFTER 'label']`.
pub(super) fn parse_add_value(text: &str) -> Result<AddValue, SqlError> {
    let (if_not_exists, text) = match strip_word(text, "if") {
        Some(tail) => (
            true,
            strip_word(tail, "not")
                .and_then(|tail| strip_word(tail, "exists"))
                .ok_or_else(|| SqlError::syntax("ADD VALUE IF requires NOT EXISTS".into()))?,
        ),
        None => (false, text),
    };
    let (label, rest) = take_label(text)?;
    let placement = if rest.is_empty() {
        None
    } else if let Some(tail) = strip_word(rest, "before") {
        Some(EnumValuePlacement::Before(take_neighbour(tail)?))
    } else if let Some(tail) = strip_word(rest, "after") {
        Some(EnumValuePlacement::After(take_neighbour(tail)?))
    } else {
        return Err(SqlError::syntax(
            "ADD VALUE expects BEFORE or AFTER after the new label".into(),
        ));
    };
    Ok(AddValue {
        label,
        if_not_exists,
        placement,
    })
}

fn take_neighbour(text: &str) -> Result<String, SqlError> {
    let (label, rest) = take_label(text)?;
    if rest.is_empty() {
        Ok(label)
    } else {
        Err(SqlError::syntax(
            "unexpected text after the neighbouring enum label".into(),
        ))
    }
}

/// Parses what follows `ALTER TYPE name RENAME VALUE`: `'from' TO 'to'`.
pub(super) fn parse_rename_value(text: &str) -> Result<(String, String), SqlError> {
    let (from, rest) = take_label(text)?;
    let rest = strip_word(rest, "to")
        .ok_or_else(|| SqlError::syntax("RENAME VALUE requires TO and a new label".into()))?;
    let (to, rest) = take_label(rest)?;
    if !rest.is_empty() {
        return Err(SqlError::syntax(
            "unexpected text after the new enum label".into(),
        ));
    }
    Ok((from, to))
}

#[cfg(test)]
#[path = "enum_labels_tests.rs"]
mod tests;
