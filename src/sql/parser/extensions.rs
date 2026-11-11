use crate::sql::ast::{ExtensionAction, ExtensionStatement, ParsedStatement, QueryStatement};

use super::SqlError;

#[derive(Debug)]
enum Token {
    Word(String),
    String(String),
}

impl Token {
    fn word(&self) -> Option<&str> {
        match self {
            Self::Word(value) => Some(value),
            Self::String(_) => None,
        }
    }
}

pub(super) fn parse_extension_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let tokens = tokenize(sql)?;
    let action = match tokens.as_slice() {
        [Token::Word(create), Token::Word(extension), rest @ ..]
            if create == "create" && extension == "extension" =>
        {
            parse_create(rest)?
        }
        [Token::Word(alter), Token::Word(extension), rest @ ..]
            if alter == "alter" && extension == "extension" =>
        {
            parse_alter(rest)?
        }
        [Token::Word(drop), Token::Word(extension), rest @ ..]
            if drop == "drop" && extension == "extension" =>
        {
            parse_drop(rest)?
        }
        _ => return Err(SqlError::syntax("invalid EXTENSION statement".to_owned())),
    };
    Ok(ParsedStatement {
        raw_sql: sql.to_owned(),
        statement: QueryStatement::Extension(ExtensionStatement { action }),
    })
}

fn parse_create(tokens: &[Token]) -> Result<ExtensionAction, SqlError> {
    let mut index = 0;
    let if_not_exists = if has_words(tokens, index, &["if", "not", "exists"]) {
        index += 3;
        true
    } else {
        false
    };
    let name = take_identifier(tokens, &mut index, "extension name")?;
    let mut schema = None;
    let mut version = None;
    let mut saw_with = false;
    while index < tokens.len() {
        match word_at(tokens, index) {
            Some("with") if !saw_with => {
                saw_with = true;
                index += 1;
            }
            Some("schema") if schema.is_none() => {
                index += 1;
                schema = Some(take_identifier(tokens, &mut index, "schema name")?);
            }
            Some("version") if version.is_none() => {
                index += 1;
                version = Some(take_string(tokens, &mut index, "extension version")?);
            }
            Some(_) => {
                return Err(SqlError::unsupported(
                    "unsupported CREATE EXTENSION option".to_owned(),
                ));
            }
            None => {
                return Err(SqlError::syntax(
                    "invalid CREATE EXTENSION option".to_owned(),
                ))
            }
        }
    }
    Ok(ExtensionAction::Create {
        name,
        if_not_exists,
        schema,
        version,
    })
}

fn parse_alter(tokens: &[Token]) -> Result<ExtensionAction, SqlError> {
    let mut index = 0;
    let name = take_identifier(tokens, &mut index, "extension name")?;
    match word_at(tokens, index) {
        Some("set") if word_at(tokens, index + 1) == Some("schema") => {
            index += 2;
            let schema = take_identifier(tokens, &mut index, "schema name")?;
            ensure_end(tokens, index)?;
            Ok(ExtensionAction::SetSchema { name, schema })
        }
        Some("update") => {
            index += 1;
            let version = if word_at(tokens, index) == Some("to") {
                index += 1;
                Some(take_string(tokens, &mut index, "extension version")?)
            } else {
                None
            };
            ensure_end(tokens, index)?;
            Ok(ExtensionAction::Update { name, version })
        }
        _ => Err(SqlError::unsupported(
            "unsupported ALTER EXTENSION operation".to_owned(),
        )),
    }
}

fn parse_drop(tokens: &[Token]) -> Result<ExtensionAction, SqlError> {
    let mut index = 0;
    let if_exists = if has_words(tokens, index, &["if", "exists"]) {
        index += 2;
        true
    } else {
        false
    };
    let name = take_identifier(tokens, &mut index, "extension name")?;
    let cascade = match word_at(tokens, index) {
        Some("cascade") => {
            index += 1;
            true
        }
        Some("restrict") => {
            index += 1;
            false
        }
        Some(_) => {
            return Err(SqlError::unsupported(
                "unsupported DROP EXTENSION behavior".to_owned(),
            ));
        }
        None => false,
    };
    ensure_end(tokens, index)?;
    Ok(ExtensionAction::Drop {
        name,
        if_exists,
        cascade,
    })
}

fn tokenize(sql: &str) -> Result<Vec<Token>, SqlError> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < sql.len() {
        let byte = sql.as_bytes()[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if byte == b'\'' {
            let (value, end) = quoted(sql, index, b'\'')?;
            tokens.push(Token::String(value));
            index = end;
            continue;
        }
        if byte == b'"' {
            let (value, end) = quoted(sql, index, b'"')?;
            tokens.push(Token::Word(value));
            index = end;
            continue;
        }
        let start = index;
        while index < sql.len() && !sql.as_bytes()[index].is_ascii_whitespace() {
            index += 1;
        }
        let word = sql[start..index].trim_end_matches(';');
        tokens.push(Token::Word(word.to_ascii_lowercase()));
    }
    Ok(tokens)
}

fn quoted(sql: &str, start: usize, quote: u8) -> Result<(String, usize), SqlError> {
    let mut value = String::new();
    let mut index = start + 1;
    while index < sql.len() {
        if sql.as_bytes()[index] == quote {
            if sql.as_bytes().get(index + 1) == Some(&quote) {
                value.push(char::from(quote));
                index += 2;
                continue;
            }
            return Ok((value, index + 1));
        }
        let ch = sql[index..]
            .chars()
            .next()
            .ok_or_else(|| SqlError::syntax("unterminated extension option".to_owned()))?;
        value.push(ch);
        index += ch.len_utf8();
    }
    Err(SqlError::syntax("unterminated extension option".to_owned()))
}

fn take_identifier(
    tokens: &[Token],
    index: &mut usize,
    description: &str,
) -> Result<String, SqlError> {
    let Some(Token::Word(value)) = tokens.get(*index) else {
        return Err(SqlError::syntax(format!("expected {description}")));
    };
    if !valid_identifier(value) {
        return Err(SqlError::unsupported(format!("unsupported {description}")));
    }
    *index += 1;
    Ok(value.clone())
}

fn take_string(tokens: &[Token], index: &mut usize, description: &str) -> Result<String, SqlError> {
    let Some(Token::String(value)) = tokens.get(*index) else {
        return Err(SqlError::syntax(format!("expected quoted {description}")));
    };
    *index += 1;
    Ok(value.clone())
}

fn valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$')
}

fn word_at(tokens: &[Token], index: usize) -> Option<&str> {
    tokens.get(index).and_then(Token::word)
}

fn has_words(tokens: &[Token], index: usize, words: &[&str]) -> bool {
    words
        .iter()
        .enumerate()
        .all(|(offset, expected)| word_at(tokens, index + offset) == Some(*expected))
}

fn ensure_end(tokens: &[Token], index: usize) -> Result<(), SqlError> {
    if index == tokens.len() {
        Ok(())
    } else {
        Err(SqlError::unsupported(
            "unsupported EXTENSION statement clause".to_owned(),
        ))
    }
}

#[cfg(test)]
#[path = "extensions_tests.rs"]
mod tests;
