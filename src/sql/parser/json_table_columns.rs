//! `COLUMNS (...)` clauses of `JSON_TABLE`: ordinality, value, `EXISTS` and `NESTED` columns.

use super::super::domain_scope::parse_declared_data_type;
use super::super::identifiers::normalize_column_identifier;
use super::super::lexical;
use super::{parse_expression, split_csv, SqlError};
use crate::sql::ast::{
    ExistsBehavior, Expr, JsonBehavior, JsonQuotes, JsonTableColumn, JsonTableExistsColumn,
    JsonTableNested, JsonTableValueColumn, JsonWrapper,
};
use crate::types::DataType;

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

impl Token<'_> {
    fn is_word(&self) -> bool {
        !matches!(self.text.as_bytes().first(), Some(b'\'' | b'"' | b'('))
    }
}

fn tokenize(definition: &str) -> Result<Vec<Token<'_>>, SqlError> {
    let mut tokens = Vec::new();
    let mut index = lexical::separator_end(definition, 0);
    while index < definition.len() {
        let end = match definition.as_bytes()[index] {
            b'\'' | b'"' => lexical::quoted_end(definition, index),
            b'(' => lexical::matching_paren(definition, index).map(|close| close + 1),
            _ => Some(
                definition[index..]
                    .find(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"' | '('))
                    .map_or(definition.len(), |offset| index + offset),
            ),
        }
        .ok_or_else(|| SqlError::syntax("unbalanced JSON_TABLE column definition".into()))?;
        tokens.push(Token {
            text: &definition[index..end],
            start: index,
            end,
        });
        index = lexical::separator_end(definition, end);
    }
    Ok(tokens)
}

/// A behavior keyword and the clause (`ON EMPTY` or `ON ERROR`) it was given for.
enum Behavior {
    Null,
    Error,
    EmptyArray,
    EmptyObject,
    Default(Expr),
    True,
    False,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Empty,
    Error,
}

struct Cursor<'a> {
    definition: &'a str,
    tokens: Vec<Token<'a>>,
    at: usize,
}

impl<'a> Cursor<'a> {
    fn word(&self, offset: usize) -> Option<String> {
        let token = self.tokens.get(self.at + offset)?;
        token.is_word().then(|| token.text.to_ascii_lowercase())
    }

    fn done(&self) -> bool {
        self.at >= self.tokens.len()
    }

    fn near(&self) -> SqlError {
        let near = self
            .tokens
            .get(self.at)
            .map_or("end of input", |token| token.text);
        SqlError::syntax(format!("syntax error at or near \"{near}\""))
    }

    fn eat(&mut self, keyword: &str) -> bool {
        if self.word(0).as_deref() == Some(keyword) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, keyword: &str) -> Result<(), SqlError> {
        if self.eat(keyword) {
            Ok(())
        } else {
            Err(self.near())
        }
    }

    fn name(&mut self) -> Result<String, SqlError> {
        let token = *self.tokens.get(self.at).ok_or_else(|| self.near())?;
        if token.text.starts_with('\'') || token.text.starts_with('(') {
            return Err(self.near());
        }
        self.at += 1;
        normalize_column_identifier(token.text)
    }

    fn string(&mut self) -> Result<String, SqlError> {
        let token = *self.tokens.get(self.at).ok_or_else(|| self.near())?;
        if !token.text.starts_with('\'') {
            return Err(SqlError::unsupported(
                "only string constants are supported in JSON_TABLE path specification".into(),
            ));
        }
        self.at += 1;
        match parse_expression(token.text)? {
            Expr::StringLiteral(text) => Ok(text),
            _ => Err(self.near()),
        }
    }

    fn group(&mut self) -> Result<&'a str, SqlError> {
        let token = *self.tokens.get(self.at).ok_or_else(|| self.near())?;
        if !token.text.starts_with('(') {
            return Err(self.near());
        }
        self.at += 1;
        Ok(&token.text[1..token.text.len() - 1])
    }

    /// Whether the token at `index` starts a clause that ends the column type.
    fn clause_start(&self, index: usize) -> bool {
        let Some(token) = self.tokens.get(index).filter(|token| token.is_word()) else {
            return false;
        };
        let word = token.text.to_ascii_lowercase();
        match word.as_str() {
            "format" | "path" | "exists" | "without" | "keep" | "omit" | "error" | "null"
            | "default" | "empty" | "true" | "false" | "unknown" => true,
            "with" => self
                .tokens
                .get(index + 1)
                .filter(|next| next.is_word())
                .is_some_and(|next| {
                    matches!(
                        next.text.to_ascii_lowercase().as_str(),
                        "wrapper" | "conditional" | "unconditional" | "array"
                    )
                }),
            _ => false,
        }
    }

    fn data_type(&mut self) -> Result<DataType, SqlError> {
        let first = self.at;
        while self.at < self.tokens.len() && !self.clause_start(self.at) {
            self.at += 1;
        }
        if self.at == first {
            return Err(self.near());
        }
        let text = &self.definition[self.tokens[first].start..self.tokens[self.at - 1].end];
        parse_declared_data_type(text)
    }

    fn wrapper(&mut self) -> Result<Option<JsonWrapper>, SqlError> {
        let wrapper = if self.eat("without") {
            JsonWrapper::Without
        } else if self.eat("with") {
            if self.eat("conditional") {
                JsonWrapper::Conditional
            } else {
                self.eat("unconditional");
                JsonWrapper::Unconditional
            }
        } else {
            return Ok(None);
        };
        self.eat("array");
        self.expect("wrapper")?;
        Ok(Some(wrapper))
    }

    fn quotes(&mut self) -> Result<Option<JsonQuotes>, SqlError> {
        let quotes = if self.eat("keep") {
            JsonQuotes::Keep
        } else if self.eat("omit") {
            JsonQuotes::Omit
        } else {
            return Ok(None);
        };
        self.expect("quotes")?;
        if self.eat("on") {
            self.expect("scalar")?;
            self.expect("string")?;
        }
        Ok(Some(quotes))
    }

    /// `behavior ON {EMPTY|ERROR}`, or `None` when the next token is not a behavior.
    fn behavior(&mut self) -> Result<Option<(Behavior, Slot)>, SqlError> {
        let Some(word) = self.word(0) else {
            return Ok(None);
        };
        let behavior = match word.as_str() {
            "null" => Behavior::Null,
            "error" => Behavior::Error,
            "true" => Behavior::True,
            "false" => Behavior::False,
            "unknown" => Behavior::Unknown,
            "empty" => {
                self.at += 1;
                let behavior = if self.eat("object") {
                    Behavior::EmptyObject
                } else {
                    self.eat("array");
                    Behavior::EmptyArray
                };
                return self.slot(behavior).map(Some);
            }
            "default" => {
                self.at += 1;
                let expression = self.default_expression()?;
                return self.slot(Behavior::Default(expression)).map(Some);
            }
            _ => return Ok(None),
        };
        self.at += 1;
        self.slot(behavior).map(Some)
    }

    fn slot(&mut self, behavior: Behavior) -> Result<(Behavior, Slot), SqlError> {
        self.expect("on")?;
        if self.eat("empty") {
            Ok((behavior, Slot::Empty))
        } else if self.eat("error") {
            Ok((behavior, Slot::Error))
        } else {
            Err(self.near())
        }
    }

    fn default_expression(&mut self) -> Result<Expr, SqlError> {
        let first = self.at;
        while self.at < self.tokens.len() {
            let is_slot_start = self.word(0).as_deref() == Some("on")
                && matches!(self.word(1).as_deref(), Some("empty" | "error"));
            if is_slot_start {
                break;
            }
            self.at += 1;
        }
        if self.at == first {
            return Err(self.near());
        }
        let text = &self.definition[self.tokens[first].start..self.tokens[self.at - 1].end];
        parse_expression(text)
    }
}

/// The default row pattern of a column, `$."name"`.
fn default_path(name: &str) -> String {
    format!("$.\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Parses the inside of a `COLUMNS (...)` clause.
pub(super) fn parse_columns(inside: &str) -> Result<Vec<JsonTableColumn>, SqlError> {
    split_csv(inside).into_iter().map(parse_column).collect()
}

fn parse_column(definition: &str) -> Result<JsonTableColumn, SqlError> {
    let mut cursor = Cursor {
        definition,
        tokens: tokenize(definition)?,
        at: 0,
    };
    // `nested` starts a NESTED clause only before `PATH` or a path literal.
    let nested_clause = cursor.word(0).as_deref() == Some("nested")
        && (cursor.word(1).as_deref() == Some("path")
            || cursor
                .tokens
                .get(cursor.at + 1)
                .is_some_and(|token| token.text.starts_with('\'')));
    if nested_clause {
        cursor.at += 1;
        return parse_nested(&mut cursor);
    }
    let name = cursor.name()?;
    if cursor.eat("for") {
        cursor.expect("ordinality")?;
        if !cursor.done() {
            return Err(cursor.near());
        }
        return Ok(JsonTableColumn::Ordinality { name });
    }
    let data_type = cursor.data_type()?;
    if cursor.eat("exists") {
        parse_exists(&mut cursor, name, data_type)
    } else {
        parse_value(&mut cursor, name, data_type)
    }
}

fn parse_nested(cursor: &mut Cursor<'_>) -> Result<JsonTableColumn, SqlError> {
    cursor.eat("path");
    let path = cursor.string()?;
    let path_name = if cursor.eat("as") {
        Some(cursor.name()?)
    } else {
        None
    };
    cursor.expect("columns")?;
    let inside = cursor.group()?;
    if !cursor.done() {
        return Err(cursor.near());
    }
    Ok(JsonTableColumn::Nested(Box::new(JsonTableNested {
        path,
        path_name,
        columns: parse_columns(inside)?,
    })))
}

fn parse_exists(
    cursor: &mut Cursor<'_>,
    name: String,
    data_type: DataType,
) -> Result<JsonTableColumn, SqlError> {
    let mut path = None;
    let mut on_error = ExistsBehavior::Unspecified;
    while !cursor.done() {
        if cursor.eat("path") {
            path = Some(cursor.string()?);
        } else if let Some((behavior, slot)) = cursor.behavior()? {
            if slot != Slot::Error {
                return Err(cursor.near());
            }
            on_error = match behavior {
                Behavior::Error => ExistsBehavior::Error,
                Behavior::True => ExistsBehavior::True,
                Behavior::False => ExistsBehavior::False,
                Behavior::Unknown => ExistsBehavior::Unknown,
                _ => {
                    return Err(SqlError::syntax(format!(
                        "invalid ON ERROR behavior for column \"{name}\": only ERROR, TRUE, \
                         FALSE, or UNKNOWN is allowed in ON ERROR for EXISTS columns"
                    )))
                }
            };
        } else {
            return Err(cursor.near());
        }
    }
    Ok(JsonTableColumn::Exists(Box::new(JsonTableExistsColumn {
        path: path.unwrap_or_else(|| default_path(&name)),
        name,
        data_type,
        on_error,
    })))
}

fn parse_value(
    cursor: &mut Cursor<'_>,
    name: String,
    data_type: DataType,
) -> Result<JsonTableColumn, SqlError> {
    let mut column = JsonTableValueColumn {
        path: String::new(),
        name,
        data_type,
        format_json: false,
        wrapper: JsonWrapper::Unspecified,
        quotes: JsonQuotes::Unspecified,
        on_empty: JsonBehavior::Unspecified,
        on_error: JsonBehavior::Unspecified,
    };
    let mut path = None;
    while !cursor.done() {
        if cursor.eat("format") {
            cursor.expect("json")?;
            if cursor.eat("encoding") {
                cursor.at += 1;
            }
            column.format_json = true;
        } else if cursor.eat("path") {
            path = Some(cursor.string()?);
        } else if let Some(wrapper) = cursor.wrapper()? {
            column.wrapper = wrapper;
        } else if let Some(quotes) = cursor.quotes()? {
            column.quotes = quotes;
        } else if let Some((behavior, slot)) = cursor.behavior()? {
            assign_behavior(&mut column, cursor, behavior, slot)?;
        } else {
            return Err(cursor.near());
        }
    }
    column.path = path.unwrap_or_else(|| default_path(&column.name));
    Ok(JsonTableColumn::Value(Box::new(column)))
}

fn assign_behavior(
    column: &mut JsonTableValueColumn,
    cursor: &Cursor<'_>,
    behavior: Behavior,
    slot: Slot,
) -> Result<(), SqlError> {
    let converted = match behavior {
        Behavior::Null => JsonBehavior::Null,
        Behavior::Error => JsonBehavior::Error,
        Behavior::EmptyArray => JsonBehavior::EmptyArray,
        Behavior::EmptyObject => JsonBehavior::EmptyObject,
        Behavior::Default(expression) => JsonBehavior::Default(expression),
        Behavior::True | Behavior::False | Behavior::Unknown => return Err(cursor.near()),
    };
    match slot {
        Slot::Empty if matches!(column.on_error, JsonBehavior::Unspecified) => {
            column.on_empty = converted;
            Ok(())
        }
        Slot::Error => {
            column.on_error = converted;
            Ok(())
        }
        Slot::Empty => Err(cursor.near()),
    }
}
