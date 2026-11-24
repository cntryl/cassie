//! Path pattern parsing for `GRAPH_TABLE` (#1256): vertex and edge elements,
//! labels, inline predicates, directions and quantifiers.
//!
//! All functions work on the text between the `GRAPH_TABLE` parentheses and
//! return byte positions into that text.

use super::expr::parse_expression;
use super::identifiers::normalize_identifier;
use super::lexical;
use super::SqlError;
use crate::ast::{
    Expr, GraphEdgeDirection, GraphEdgePattern, GraphPathPattern, GraphPathStep, GraphQuantifier,
    GraphVertexPattern,
};

pub(super) fn skip(text: &str, pos: usize) -> usize {
    lexical::separator_end(text, pos)
}

pub(super) fn byte_at(text: &str, pos: usize) -> Option<u8> {
    text.as_bytes().get(pos).copied()
}

/// End of the identifier word starting at `start`, honoring quoted identifiers.
/// A `.` continues the word only when `dotted` is set (qualified graph names).
pub(super) fn word_end(text: &str, start: usize, dotted: bool) -> usize {
    let mut index = start;
    while index < text.len() {
        if byte_at(text, index) == Some(b'"') {
            let Some(end) = lexical::quoted_end(text, index) else {
                break;
            };
            index = end;
            continue;
        }
        let Some(character) = text.get(index..).and_then(|tail| tail.chars().next()) else {
            break;
        };
        if lexical::identifier_character(character) || (dotted && character == '.') {
            index += character.len_utf8();
        } else {
            break;
        }
    }
    index
}

/// A syntax error naming the token found at `pos`.
pub(super) fn unexpected(text: &str, pos: usize, expected: &str) -> SqlError {
    let found = text
        .get(pos..)
        .and_then(|tail| tail.split_whitespace().next())
        .map_or_else(
            || "end of GRAPH_TABLE".to_owned(),
            |token| format!("\"{token}\""),
        );
    SqlError::syntax(format!("GRAPH_TABLE expected {expected}, found {found}"))
}

struct Header {
    variable: Option<String>,
    labels: Vec<String>,
    filter: Option<Expr>,
}

fn label_operator(byte: u8) -> bool {
    matches!(byte, b'&' | b'!' | b'%' | b'(' | b':' | b'+')
}

fn unsupported_label_expression() -> SqlError {
    SqlError::unsupported("GRAPH_TABLE label expressions support only `|` alternation".into())
}

fn parse_labels(text: &str, mut pos: usize) -> Result<(Vec<String>, usize), SqlError> {
    let mut labels = Vec::new();
    loop {
        let end = word_end(text, pos, false);
        if end == pos {
            return Err(match byte_at(text, pos) {
                Some(byte) if label_operator(byte) => unsupported_label_expression(),
                _ => unexpected(text, pos, "a label name"),
            });
        }
        labels.push(normalize_identifier(&text[pos..end])?);
        pos = skip(text, end);
        if byte_at(text, pos) == Some(b'|') {
            pos = skip(text, pos + 1);
        } else {
            return Ok((labels, pos));
        }
    }
}

/// `[variable] [IS label {| label}] [WHERE predicate]` between an element's
/// delimiters.
fn parse_header(inner: &str) -> Result<Header, SqlError> {
    let mut pos = skip(inner, 0);
    let mut header = Header {
        variable: None,
        labels: Vec::new(),
        filter: None,
    };
    if pos < inner.len()
        && lexical::pattern_end(inner, pos, "is").is_none()
        && lexical::pattern_end(inner, pos, "where").is_none()
    {
        let end = word_end(inner, pos, false);
        if end == pos {
            return Err(unexpected(inner, pos, "an element variable"));
        }
        header.variable = Some(normalize_identifier(&inner[pos..end])?);
        pos = skip(inner, end);
    }
    if let Some(end) = lexical::pattern_end(inner, pos, "is") {
        let (labels, next) = parse_labels(inner, skip(inner, end))?;
        header.labels = labels;
        pos = next;
    }
    if let Some(end) = lexical::pattern_end(inner, pos, "where") {
        let predicate = lexical::trim_separators(&inner[end..]);
        if predicate.is_empty() {
            return Err(SqlError::syntax(
                "GRAPH_TABLE inline WHERE requires a predicate".into(),
            ));
        }
        header.filter = Some(parse_expression(predicate)?);
        return Ok(header);
    }
    match byte_at(inner, pos) {
        None => Ok(header),
        Some(byte) if label_operator(byte) => Err(unsupported_label_expression()),
        Some(_) => Err(unexpected(
            inner,
            pos,
            "IS, WHERE or the end of the element",
        )),
    }
}

fn parse_vertex(text: &str, pos: usize) -> Result<(GraphVertexPattern, usize), SqlError> {
    if byte_at(text, pos) != Some(b'(') {
        return Err(unexpected(
            text,
            pos,
            "a vertex pattern starting with \"(\"",
        ));
    }
    let close = lexical::matching_paren(text, pos)
        .ok_or_else(|| SqlError::syntax("GRAPH_TABLE vertex pattern is missing \")\"".into()))?;
    let header = parse_header(&text[pos + 1..close])?;
    Ok((
        GraphVertexPattern {
            variable: header.variable,
            labels: header.labels,
            filter: header.filter,
        },
        close + 1,
    ))
}

fn nested_quantifier() -> SqlError {
    SqlError::unsupported(
        "GRAPH_TABLE supports one quantifier per edge; parenthesized repetition, quantified \
         vertices and nested quantifiers are not supported"
            .into(),
    )
}

fn parse_bound(text: &str) -> Result<u32, SqlError> {
    if text.is_empty() {
        return Err(SqlError::syntax(
            "GRAPH_TABLE quantifier bound must be an unsigned integer".into(),
        ));
    }
    if text.starts_with('$') {
        return Err(SqlError::unsupported(
            "GRAPH_TABLE quantifier bounds cannot be parameters".into(),
        ));
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SqlError::syntax(format!(
            "GRAPH_TABLE quantifier bound \"{text}\" is not an unsigned integer"
        )));
    }
    text.parse::<u32>().map_err(|_| {
        SqlError::invalid_parameter_value(format!(
            "GRAPH_TABLE quantifier bound \"{text}\" is out of range"
        ))
    })
}

fn parse_braces(text: &str, pos: usize) -> Result<(GraphQuantifier, usize), SqlError> {
    let close = text[pos..]
        .find('}')
        .map(|offset| pos + offset)
        .ok_or_else(|| SqlError::syntax("GRAPH_TABLE quantifier is missing \"}\"".into()))?;
    let parts: Vec<&str> = text[pos + 1..close].split(',').map(str::trim).collect();
    let quantifier = match parts.as_slice() {
        [exact] => GraphQuantifier::Exact(parse_bound(exact)?),
        [min, max] if min.is_empty() || max.is_empty() => {
            return Err(SqlError::unsupported(
                "GRAPH_TABLE open-ended quantifiers {m,} and {,n} are not supported".into(),
            ));
        }
        [min, max] => {
            let (min, max) = (parse_bound(min)?, parse_bound(max)?);
            if min > max {
                return Err(SqlError::invalid_parameter_value(format!(
                    "GRAPH_TABLE quantifier lower bound {min} exceeds upper bound {max}"
                )));
            }
            GraphQuantifier::Range { min, max }
        }
        _ => {
            return Err(SqlError::syntax(
                "GRAPH_TABLE quantifier has too many bounds".into(),
            ));
        }
    };
    Ok((quantifier, close + 1))
}

/// An optional quantifier at `pos`, with the position after it.
fn parse_quantifier(text: &str, pos: usize) -> Result<(Option<GraphQuantifier>, usize), SqlError> {
    let (quantifier, end) = match byte_at(text, pos) {
        Some(b'*') => (GraphQuantifier::ZeroOrMore, pos + 1),
        Some(b'+') => (GraphQuantifier::OneOrMore, pos + 1),
        Some(b'?') => {
            return Err(SqlError::unsupported(
                "GRAPH_TABLE quantifier `?` is not supported".into(),
            ));
        }
        Some(b'{') => parse_braces(text, pos)?,
        _ => return Ok((None, pos)),
    };
    if matches!(
        byte_at(text, skip(text, end)),
        Some(b'{' | b'*' | b'+' | b'?')
    ) {
        return Err(nested_quantifier());
    }
    Ok((Some(quantifier), end))
}

fn expect_byte(text: &str, pos: usize, byte: u8, expected: &str) -> Result<usize, SqlError> {
    if byte_at(text, pos) == Some(byte) {
        Ok(pos + 1)
    } else {
        Err(unexpected(text, pos, expected))
    }
}

/// `- [..] ->`, `<- [..] -` or `- [..] -`, plus an optional quantifier.
fn parse_edge(text: &str, start: usize) -> Result<(GraphEdgePattern, usize), SqlError> {
    let incoming = byte_at(text, start) == Some(b'<');
    let mut pos = start;
    if incoming {
        pos = skip(text, pos + 1);
    }
    pos = skip(text, expect_byte(text, pos, b'-', "\"-\"")?);
    if byte_at(text, pos) != Some(b'[') {
        return Err(unexpected(
            text,
            pos,
            "\"[\" (abbreviated edge patterns are not supported)",
        ));
    }
    let close = lexical::matching_bracket(text, pos)
        .ok_or_else(|| SqlError::syntax("GRAPH_TABLE edge pattern is missing \"]\"".into()))?;
    let header = parse_header(&text[pos + 1..close])?;
    pos = expect_byte(text, skip(text, close + 1), b'-', "\"-\" or \"->\"")?;
    let direction = if byte_at(text, pos) == Some(b'>') {
        if incoming {
            return Err(SqlError::unsupported(
                "GRAPH_TABLE bidirectional edge patterns `<-[..]->` are not supported".into(),
            ));
        }
        pos += 1;
        GraphEdgeDirection::Outgoing
    } else if incoming {
        GraphEdgeDirection::Incoming
    } else {
        GraphEdgeDirection::Undirected
    };
    let (quantifier, end) = parse_quantifier(text, skip(text, pos))?;
    Ok((
        GraphEdgePattern {
            variable: header.variable,
            labels: header.labels,
            filter: header.filter,
            direction,
            quantifier,
        },
        end,
    ))
}

/// `vertex {edge [quantifier] vertex}` starting at `pos`; returns the position
/// after the last element so the caller can inspect what follows.
pub(super) fn parse_path(text: &str, pos: usize) -> Result<(GraphPathPattern, usize), SqlError> {
    let (start, mut pos) = parse_vertex(text, skip(text, pos))?;
    let mut steps = Vec::new();
    loop {
        pos = skip(text, pos);
        match byte_at(text, pos) {
            Some(b'-' | b'<') => {
                let (edge, next) = parse_edge(text, pos)?;
                let (vertex, next) = parse_vertex(text, skip(text, next))?;
                steps.push(GraphPathStep { edge, vertex });
                pos = next;
            }
            Some(b'{' | b'*' | b'+' | b'?') => return Err(nested_quantifier()),
            _ => return Ok((GraphPathPattern { start, steps }, pos)),
        }
    }
}
