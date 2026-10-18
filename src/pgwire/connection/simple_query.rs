use std::str::Chars;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SplitError {
    Syntax(String),
    Unsupported(String),
    NumericRange(String),
    DuplicateColumn(String),
    ResourceLimit(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexState {
    Normal,
    SingleQuoted(bool),
    DoubleQuoted,
    LineComment,
    BlockComment(usize),
}

pub(super) fn split_simple_query(sql: &str) -> Result<Vec<String>, SplitError> {
    crate::sql::parser::preflight_sql(sql).map_err(|error| match error.kind() {
        crate::sql::SqlErrorKind::ResourceLimit => SplitError::ResourceLimit(error.to_string()),
        crate::sql::SqlErrorKind::NumericRange => SplitError::NumericRange(error.to_string()),
        crate::sql::SqlErrorKind::DuplicateColumn => SplitError::DuplicateColumn(error.to_string()),
        crate::sql::SqlErrorKind::Syntax => SplitError::Syntax(error.to_string()),
        crate::sql::SqlErrorKind::Unsupported => SplitError::Unsupported(error.to_string()),
    })?;
    let mut state = LexState::Normal;
    let mut current = String::new();
    let mut statements = Vec::new();
    let mut requires_normalization = false;
    let mut chars = sql.chars();

    loop {
        let start = sql.len() - chars.as_str().len();
        let Some(character) = chars.next() else {
            break;
        };
        if matches!(state, LexState::Normal) && character == '$' {
            if let Some(end) = crate::sql::parser::dollar_quoted_end(sql, start) {
                let literal = &sql[start..end];
                if crate::sql::parser::dollar_quoted_content(literal).is_none() {
                    return Err(SplitError::Syntax(
                        "unterminated dollar-quoted string in simple query".to_owned(),
                    ));
                }
                current.push_str(literal);
                chars = sql[end..].chars();
                continue;
            }
        }
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
        LexState::SingleQuoted(_) => {
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
    chars: &mut Chars<'_>,
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
        LexState::SingleQuoted(escapes) => {
            consume_quoted_character(character, chars, state, current, '\'', escapes);
        }
        LexState::DoubleQuoted => {
            consume_quoted_character(character, chars, state, current, '"', false);
        }
        LexState::LineComment => {
            if character == '\n' || character == '\r' {
                current.push(character);
                *state = LexState::Normal;
            }
        }
        LexState::BlockComment(depth) => {
            if character == '/' && consume_if(chars, '*') {
                *state = LexState::BlockComment(depth.saturating_add(1));
            } else if character == '*' && consume_if(chars, '/') {
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
    chars: &mut Chars<'_>,
    state: &mut LexState,
    current: &mut String,
    statements: &mut Vec<String>,
    requires_normalization: &mut bool,
) {
    match character {
        '\'' => {
            let escapes = crate::sql::parser::escape_string_prefix(current, current.len());
            current.push(character);
            *state = LexState::SingleQuoted(escapes);
        }
        '"' => {
            current.push(character);
            *state = LexState::DoubleQuoted;
        }
        '-' if consume_if(chars, '-') => {
            current.push(' ');
            *requires_normalization = true;
            *state = LexState::LineComment;
        }
        '/' if consume_if(chars, '*') => {
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
    chars: &mut Chars<'_>,
    state: &mut LexState,
    current: &mut String,
    quote: char,
    escapes: bool,
) {
    current.push(character);
    if escapes && character == '\\' {
        if let Some(escaped) = chars.next() {
            current.push(escaped);
        }
    } else if character == quote {
        if consume_if(chars, quote) {
            current.push(quote);
        } else {
            *state = LexState::Normal;
        }
    }
}

fn consume_if(chars: &mut Chars<'_>, expected: char) -> bool {
    chars.as_str().starts_with(expected) && chars.next().is_some()
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
    fn should_ignore_statement_delimiters_inside_sql_regions() {
        // Arrange
        let sql = "SELECT ';' /* ; */; SELECT 2 -- ;\n";

        // Act
        let statements = split_simple_query(sql).expect("split query");

        // Assert
        assert_eq!(statements, vec!["SELECT ';'", "SELECT 2"]);
    }

    #[test]
    fn should_preserve_dollar_quoted_regions_given_literal_delimiters_when_splitting_real_statements(
    ) {
        // Arrange
        let sql = "SELECT $té$it's; /* untouched */ (END)$té$ AS value; \
                   SELECT $$next; ' -- literal$$";

        // Act
        let statements = split_simple_query(sql).expect("split outside dollar-quoted literals");

        // Assert
        assert_eq!(
            statements,
            vec![
                "SELECT $té$it's; /* untouched */ (END)$té$ AS value",
                "SELECT $$next; ' -- literal$$",
            ]
        );
    }

    #[test]
    fn should_reject_unterminated_dollar_quotes_given_an_earlier_statement_when_splitting_the_batch(
    ) {
        // Arrange
        let sql = "SELECT 1; SELECT $tag$unterminated";

        // Act
        let result = split_simple_query(sql);

        // Assert
        assert!(matches!(
            result,
            Err(SplitError::Syntax(message)) if message.contains("unterminated dollar-quoted")
        ));
    }
}
