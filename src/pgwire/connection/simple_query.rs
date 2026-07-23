use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SplitError {
    Syntax(String),
    Unsupported(String),
    ResourceLimit(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexState {
    Normal,
    SingleQuoted,
    DoubleQuoted,
    LineComment,
    BlockComment(usize),
}

pub(super) fn split_simple_query(sql: &str) -> Result<Vec<String>, SplitError> {
    crate::sql::parser::preflight_sql(sql).map_err(|error| match error.kind() {
        crate::sql::SqlErrorKind::ResourceLimit => SplitError::ResourceLimit(error.to_string()),
        crate::sql::SqlErrorKind::Syntax => SplitError::Syntax(error.to_string()),
        crate::sql::SqlErrorKind::Unsupported => SplitError::Unsupported(error.to_string()),
    })?;
    let mut state = LexState::Normal;
    let mut current = String::new();
    let mut statements = Vec::new();
    let mut requires_normalization = false;
    let mut chars = sql.chars().peekable();

    while let Some(character) = chars.next() {
        consume_character(
            character,
            &mut chars,
            &mut state,
            &mut current,
            &mut statements,
            &mut requires_normalization,
        );
    }

    match state {
        LexState::Normal | LexState::LineComment => {}
        LexState::SingleQuoted => {
            return Err(SplitError::Syntax(
                "unterminated quoted string in simple query".to_string(),
            ));
        }
        LexState::DoubleQuoted => {
            return Err(SplitError::Syntax(
                "unterminated quoted identifier in simple query".to_string(),
            ));
        }
        LexState::BlockComment(_) => {
            return Err(SplitError::Syntax(
                "unterminated block comment in simple query".to_string(),
            ));
        }
    }
    push_statement(&mut statements, &mut current);
    if statements.len() > 256 {
        return Err(SplitError::ResourceLimit(
            "simple query statement count exceeds 256".to_string(),
        ));
    }

    if !requires_normalization {
        return if sql.trim().is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![sql.to_string()])
        };
    }

    if statements.len() > 1
        && statements
            .iter()
            .any(|statement| is_streaming_copy(statement))
    {
        return Err(SplitError::Unsupported(
            "COPY, BACKUP, and RESTORE cannot be combined with other simple-query statements"
                .to_string(),
        ));
    }

    Ok(statements)
}

fn consume_character(
    character: char,
    chars: &mut Peekable<Chars<'_>>,
    state: &mut LexState,
    current: &mut String,
    statements: &mut Vec<String>,
    requires_normalization: &mut bool,
) {
    match *state {
        LexState::Normal => consume_normal_character(
            character,
            chars,
            state,
            current,
            statements,
            requires_normalization,
        ),
        LexState::SingleQuoted => consume_quoted_character(character, chars, state, current, '\''),
        LexState::DoubleQuoted => consume_quoted_character(character, chars, state, current, '"'),
        LexState::LineComment => {
            if character == '\n' || character == '\r' {
                current.push(character);
                *state = LexState::Normal;
            }
        }
        LexState::BlockComment(depth) => {
            if character == '/' && chars.next_if_eq(&'*').is_some() {
                *state = LexState::BlockComment(depth.saturating_add(1));
            } else if character == '*' && chars.next_if_eq(&'/').is_some() {
                if depth == 1 {
                    current.push(' ');
                    *state = LexState::Normal;
                } else {
                    *state = LexState::BlockComment(depth - 1);
                }
            }
        }
    }
}

fn consume_normal_character(
    character: char,
    chars: &mut Peekable<Chars<'_>>,
    state: &mut LexState,
    current: &mut String,
    statements: &mut Vec<String>,
    requires_normalization: &mut bool,
) {
    match character {
        '\'' => {
            current.push(character);
            *state = LexState::SingleQuoted;
        }
        '"' => {
            current.push(character);
            *state = LexState::DoubleQuoted;
        }
        '-' if chars.next_if_eq(&'-').is_some() => {
            current.push(' ');
            *requires_normalization = true;
            *state = LexState::LineComment;
        }
        '/' if chars.next_if_eq(&'*').is_some() => {
            current.push(' ');
            *requires_normalization = true;
            *state = LexState::BlockComment(1);
        }
        ';' => {
            *requires_normalization = true;
            push_statement(statements, current);
        }
        _ => current.push(character),
    }
}

fn consume_quoted_character(
    character: char,
    chars: &mut Peekable<Chars<'_>>,
    state: &mut LexState,
    current: &mut String,
    quote: char,
) {
    current.push(character);
    if character == '\\' {
        if let Some(escaped) = chars.next() {
            current.push(escaped);
        }
    } else if character == quote {
        if chars.next_if_eq(&quote).is_some() {
            current.push(quote);
        } else {
            *state = LexState::Normal;
        }
    }
}

pub(super) fn is_streaming_copy(statement: &str) -> bool {
    statement
        .split_ascii_whitespace()
        .next()
        .is_some_and(|keyword| {
            keyword.eq_ignore_ascii_case("copy")
                || keyword.eq_ignore_ascii_case("backup")
                || keyword.eq_ignore_ascii_case("restore")
        })
}

fn push_statement(statements: &mut Vec<String>, current: &mut String) {
    let statement = current.trim();
    if !statement.is_empty() {
        statements.push(statement.to_string());
    }
    current.clear();
}

#[cfg(test)]
mod tests {
    use super::{split_simple_query, SplitError};

    #[test]
    fn should_reject_simple_query_batches_over_two_hundred_fifty_six_statements() {
        // Arrange
        let sql = "SELECT 1;".repeat(257);

        // Act
        let error = split_simple_query(&sql).expect_err("statement count limit");

        // Assert
        assert!(matches!(
            error,
            SplitError::ResourceLimit(message) if message.contains("statement count")
        ));
    }

    #[test]
    fn should_ignore_statement_delimiters_in_strings_and_comments() {
        // Arrange
        let sql = "SELECT ';' /* ; */; SELECT 2 -- ;\n";

        // Act
        let statements = split_simple_query(sql).expect("split query");

        // Assert
        assert_eq!(statements, vec!["SELECT ';'", "SELECT 2"]);
    }
}
