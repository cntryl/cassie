use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SplitError {
    Syntax(String),
    Unsupported(String),
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
