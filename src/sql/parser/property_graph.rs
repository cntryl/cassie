//! Parser for the finite property graph definition grammar (#1255).
//!
//! ```text
//! create_graph = CREATE PROPERTY GRAPH qualified_name
//!                VERTEX TABLES (vertex_mapping {, vertex_mapping})
//!                EDGE TABLES (edge_mapping {, edge_mapping})
//! drop_graph   = DROP PROPERTY GRAPH [IF EXISTS] qualified_name
//! rename_graph = ALTER PROPERTY GRAPH qualified_name RENAME TO new_name
//! ```

use super::{ParsedStatement, QueryStatement, SqlError};
use crate::sql::ast::{
    CreatePropertyGraph, GraphEdgeMapping, GraphEndpoint, GraphLabel, GraphProperty,
    GraphVertexMapping, IdentifierPath, PropertyGraphAction, PropertyGraphStatement,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// An identifier or dotted identifier path, exactly as written.
    Word(String),
    Open,
    Close,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, SqlError> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut characters = input.chars().peekable();
    let flush = |word: &mut String, tokens: &mut Vec<Token>| {
        if !word.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    while let Some(character) = characters.next() {
        match character {
            '"' => {
                word.push('"');
                loop {
                    match characters.next() {
                        Some('"') if characters.peek() == Some(&'"') => {
                            characters.next();
                            word.push_str("\"\"");
                        }
                        Some('"') => {
                            word.push('"');
                            break;
                        }
                        Some(inner) => word.push(inner),
                        None => {
                            return Err(SqlError::syntax(
                                "unterminated quoted identifier in property graph definition"
                                    .into(),
                            ))
                        }
                    }
                }
            }
            '(' | ')' | ',' => {
                flush(&mut word, &mut tokens);
                tokens.push(match character {
                    '(' => Token::Open,
                    ')' => Token::Close,
                    _ => Token::Comma,
                });
            }
            character if character.is_whitespace() => flush(&mut word, &mut tokens),
            character if character.is_alphanumeric() || matches!(character, '_' | '$' | '.') => {
                word.push(character);
            }
            other => {
                return Err(SqlError::syntax(format!(
                    "unexpected character '{other}' in property graph definition"
                )))
            }
        }
    }
    flush(&mut word, &mut tokens);
    Ok(tokens)
}

struct Cursor {
    tokens: Vec<Token>,
    position: usize,
}

impl Cursor {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn at_end(&self) -> bool {
        self.position >= self.tokens.len()
    }

    fn is_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(word)) if word.eq_ignore_ascii_case(keyword))
    }

    fn take_keyword(&mut self, keyword: &str) -> bool {
        let found = self.is_keyword(keyword);
        if found {
            self.position += 1;
        }
        found
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), SqlError> {
        if self.take_keyword(keyword) {
            Ok(())
        } else {
            Err(self.expected(keyword))
        }
    }

    fn expect(&mut self, token: &Token, description: &str) -> Result<(), SqlError> {
        if self.peek() == Some(token) {
            self.position += 1;
            Ok(())
        } else {
            Err(self.expected(description))
        }
    }

    fn expected(&self, description: &str) -> SqlError {
        let found = match self.peek() {
            Some(Token::Word(word)) => format!("\"{word}\""),
            Some(Token::Open) => "\"(\"".to_owned(),
            Some(Token::Close) => "\")\"".to_owned(),
            Some(Token::Comma) => "\",\"".to_owned(),
            None => "end of statement".to_owned(),
        };
        SqlError::syntax(format!(
            "property graph definition expected {description}, found {found}"
        ))
    }

    fn word(&mut self, description: &str) -> Result<String, SqlError> {
        match self.peek() {
            Some(Token::Word(word)) => {
                let word = word.clone();
                self.position += 1;
                Ok(word)
            }
            _ => Err(self.expected(description)),
        }
    }

    /// An optionally schema-qualified name, folded by the ordinary rules.
    fn qualified_name(&mut self, description: &str) -> Result<IdentifierPath, SqlError> {
        let raw = self.word(description)?;
        IdentifierPath::parse(&raw).map_err(SqlError::syntax)
    }

    /// One identifier with no qualification.
    fn identifier(&mut self, description: &str) -> Result<String, SqlError> {
        let path = self.qualified_name(description)?;
        match path.components() {
            [component] => Ok(component.value().to_owned()),
            _ => Err(SqlError::syntax(format!(
                "property graph definition expected an unqualified {description}"
            ))),
        }
    }

    fn list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, SqlError>,
    ) -> Result<Vec<T>, SqlError> {
        self.expect(&Token::Open, "\"(\"")?;
        let mut items = vec![item(self)?];
        while self.peek() == Some(&Token::Comma) {
            self.position += 1;
            items.push(item(self)?);
        }
        self.expect(&Token::Close, "\")\"")?;
        Ok(items)
    }

    fn columns(&mut self) -> Result<Vec<String>, SqlError> {
        self.list(|cursor| cursor.identifier("column name"))
    }

    fn labels(&mut self) -> Result<Vec<GraphLabel>, SqlError> {
        let mut labels = Vec::new();
        while self.take_keyword("label") {
            let name = self.identifier("label name")?;
            self.expect_keyword("properties")?;
            let properties = self.list(|cursor| {
                let column = cursor.identifier("property column")?;
                let name = if cursor.take_keyword("as") {
                    cursor.identifier("property name")?
                } else {
                    column.clone()
                };
                Ok(GraphProperty { column, name })
            })?;
            labels.push(GraphLabel { name, properties });
        }
        if labels.is_empty() {
            return Err(self.expected("LABEL"));
        }
        Ok(labels)
    }

    fn endpoint(&mut self, keyword: &str) -> Result<GraphEndpoint, SqlError> {
        self.expect_keyword(keyword)?;
        self.expect_keyword("key")?;
        let columns = self.columns()?;
        self.expect_keyword("references")?;
        let vertex_alias = self.identifier("vertex alias")?;
        let references = self.columns()?;
        Ok(GraphEndpoint {
            columns,
            vertex_alias,
            references,
        })
    }

    fn mapping_head(&mut self) -> Result<(IdentifierPath, String, Vec<String>), SqlError> {
        let table = self.qualified_name("table name")?;
        self.expect_keyword("as")?;
        let alias = self.identifier("mapping alias")?;
        self.expect_keyword("key")?;
        let key = self.columns()?;
        Ok((table, alias, key))
    }

    fn vertex(&mut self) -> Result<GraphVertexMapping, SqlError> {
        let (table, alias, key) = self.mapping_head()?;
        Ok(GraphVertexMapping {
            table,
            alias,
            key,
            labels: self.labels()?,
        })
    }

    fn edge(&mut self) -> Result<GraphEdgeMapping, SqlError> {
        let (table, alias, key) = self.mapping_head()?;
        let source = self.endpoint("source")?;
        let destination = self.endpoint("destination")?;
        Ok(GraphEdgeMapping {
            table,
            alias,
            key,
            source,
            destination,
            labels: self.labels()?,
        })
    }

    fn finish(&self) -> Result<(), SqlError> {
        if self.at_end() {
            Ok(())
        } else {
            Err(self.expected("end of statement"))
        }
    }
}

fn create(cursor: &mut Cursor) -> Result<PropertyGraphAction, SqlError> {
    let name = cursor.qualified_name("graph name")?;
    cursor.expect_keyword("vertex")?;
    cursor.expect_keyword("tables")?;
    let vertices = cursor.list(Cursor::vertex)?;
    cursor.expect_keyword("edge")?;
    cursor.expect_keyword("tables")?;
    let edges = cursor.list(Cursor::edge)?;
    Ok(PropertyGraphAction::Create(CreatePropertyGraph {
        name,
        vertices,
        edges,
    }))
}

fn drop(cursor: &mut Cursor) -> Result<PropertyGraphAction, SqlError> {
    let if_exists = if cursor.take_keyword("if") {
        cursor.expect_keyword("exists")?;
        true
    } else {
        false
    };
    let name = cursor.qualified_name("graph name")?;
    if cursor.is_keyword("cascade") || cursor.is_keyword("restrict") {
        return Err(SqlError::unsupported(
            "DROP PROPERTY GRAPH does not support CASCADE or RESTRICT".into(),
        ));
    }
    Ok(PropertyGraphAction::Drop { name, if_exists })
}

fn alter(cursor: &mut Cursor) -> Result<PropertyGraphAction, SqlError> {
    let name = cursor.qualified_name("graph name")?;
    if !cursor.take_keyword("rename") {
        return Err(SqlError::unsupported(
            "ALTER PROPERTY GRAPH supports only RENAME TO".into(),
        ));
    }
    cursor.expect_keyword("to")?;
    let new_name = cursor.identifier("new graph name")?;
    Ok(PropertyGraphAction::Rename { name, new_name })
}

/// Parses a property graph statement, or `None` when `lower` is another kind.
pub(super) fn parse_property_graph_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    type Action = fn(&mut Cursor) -> Result<PropertyGraphAction, SqlError>;
    let verbs: [(&str, Action); 3] = [("create", create), ("drop", drop), ("alter", alter)];
    for (verb, parse) in verbs {
        let mut words = lower.split_whitespace();
        if words.next() != Some(verb)
            || words.next() != Some("property")
            || words.next() != Some("graph")
        {
            continue;
        }
        let mut cursor = Cursor {
            tokens: tokenize(trimmed)?,
            position: 3,
        };
        let action = parse(&mut cursor)?;
        cursor.finish()?;
        return Ok(Some(ParsedStatement {
            raw_sql: trimmed.to_string(),
            statement: QueryStatement::PropertyGraph(PropertyGraphStatement { action }),
        }));
    }
    Ok(None)
}

#[cfg(test)]
#[path = "property_graph_tests.rs"]
mod tests;
