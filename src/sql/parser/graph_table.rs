//! Parser for the `GRAPH_TABLE` table source (#1256).
//!
//! ```text
//! graph_table = GRAPH_TABLE ( graph_name MATCH [path_variable =] path
//!                            [KEEP selector] [WHERE sql_predicate]
//!                            COLUMNS (projection {, projection}) ) [AS alias]
//! selector    = ALL [mode] PATHS | ANY SHORTEST [mode] PATH
//!             | SHORTEST k [mode] PATHS
//! mode        = WALK | TRAIL | ACYCLIC | SIMPLE
//! projection  = sql_expression [AS column_alias]
//! ```
//!
//! The path grammar lives in `graph_table_path`. Predicates and projections
//! reuse the ordinary expression parser. Binding, `k`/bound semantics beyond
//! the checks below, unbounded-`WALK` rejection and execution are the binder's
//! and executor's concern, not the parser's.

use super::expr::{parse_expression, split_csv};
use super::graph_table_path::{byte_at, parse_path, skip, unexpected, word_end};
use super::identifiers::{normalize_column_identifier, normalize_identifier};
use super::lexical;
use super::SqlError;
use crate::sql::ast::{
    Expr, GraphColumn, GraphPathMode, GraphSelector, GraphSelectorKind, GraphTable, IdentifierPath,
};

/// Words that start graph mutation or path unions. They are recognized so the
/// error is `0A000` rather than a generic syntax error.
const MUTATION_WORDS: [&str; 7] = [
    "insert", "delete", "update", "set", "create", "merge", "remove",
];

/// Parses `GRAPH_TABLE ( ... )` from the start of `source` (leading separators
/// are skipped) and returns the table with the byte length consumed.
///
/// The consumed length ends immediately after the closing parenthesis, so
/// `&source[consumed..]` is the unparsed tail where the FROM-clause parser
/// finds an optional `[AS] alias [(column aliases)]`, a join, a comma, or
/// the next clause. The alias is not part of the returned [`GraphTable`].
///
/// Errors: malformed text is `42601`; recognized but unsupported constructs
/// (comma-separated patterns, pattern unions, mutation, `?`, `{m,}`, `{,n}`,
/// nested quantifiers, bidirectional edges, label operators other than `|`,
/// parameterized bounds) are `0A000`; a quantifier with `m > n`, an
/// out-of-range bound, or `SHORTEST 0` is `22023`.
pub(crate) fn parse_graph_table(source: &str) -> Result<(GraphTable, usize), SqlError> {
    let start = skip(source, 0);
    let Some(after_keyword) = lexical::pattern_end(source, start, "graph_table") else {
        return Err(unexpected(source, start, "GRAPH_TABLE"));
    };
    let open = skip(source, after_keyword);
    let close = lexical::matching_paren(source, open).ok_or_else(|| {
        SqlError::syntax("GRAPH_TABLE requires a parenthesized MATCH ... COLUMNS body".into())
    })?;
    let table = parse_body(&source[open + 1..close])?;
    Ok((table, close + 1))
}

fn parse_body(body: &str) -> Result<GraphTable, SqlError> {
    let name_start = skip(body, 0);
    let name_end = word_end(body, name_start, true);
    if name_end == name_start {
        return Err(unexpected(body, name_start, "a graph name"));
    }
    let graph = IdentifierPath::parse(&body[name_start..name_end]).map_err(SqlError::syntax)?;
    let Some(after_match) = lexical::pattern_end(body, skip(body, name_end), "match") else {
        return Err(unexpected(body, skip(body, name_end), "MATCH"));
    };
    let pos = skip(body, after_match);
    reject_unsupported(body, pos)?;
    let (path_variable, pos) = parse_path_variable(body, pos)?;
    let (path, end) = parse_path(body, pos)?;
    let rest = &body[end..];
    reject_unsupported(rest, skip(rest, 0))?;
    let (head, columns) = split_columns(rest)?;
    let (selector, filter) = parse_head(head)?;
    Ok(GraphTable {
        graph,
        path_variable,
        path,
        selector,
        filter,
        columns: parse_columns(columns)?,
    })
}

fn parse_path_variable(body: &str, pos: usize) -> Result<(Option<String>, usize), SqlError> {
    let end = word_end(body, pos, false);
    if end == pos {
        return Ok((None, pos));
    }
    let after = skip(body, end);
    if byte_at(body, after) == Some(b'=') {
        Ok((Some(normalize_identifier(&body[pos..end])?), after + 1))
    } else {
        Err(unexpected(body, pos, "a path pattern starting with \"(\""))
    }
}

/// Rejects comma-separated patterns, unions and mutation at `pos`.
fn reject_unsupported(text: &str, pos: usize) -> Result<(), SqlError> {
    match byte_at(text, pos) {
        Some(b',') => {
            return Err(SqlError::unsupported(
                "comma-separated GRAPH_TABLE path patterns are not supported".into(),
            ));
        }
        Some(b'|') => {
            return Err(SqlError::unsupported(
                "GRAPH_TABLE path pattern unions are not supported".into(),
            ));
        }
        _ => {}
    }
    let word = text[pos..word_end(text, pos, false)].to_ascii_lowercase();
    if word == "union" {
        return Err(SqlError::unsupported(
            "GRAPH_TABLE path pattern unions are not supported".into(),
        ));
    }
    if MUTATION_WORDS.contains(&word.as_str()) {
        return Err(SqlError::unsupported(
            "graph mutation is not supported in GRAPH_TABLE".into(),
        ));
    }
    Ok(())
}

/// Splits what follows the path at its `COLUMNS` keyword, skipping a
/// qualified column such as `t.columns`.
fn split_columns(rest: &str) -> Result<(&str, &str), SqlError> {
    let mut from = 0;
    while let Some((start, end)) = lexical::find_top_level_span(rest, from, "columns") {
        if rest[..start].trim_end().ends_with('.') {
            from = end;
        } else {
            return Ok((&rest[..start], &rest[end..]));
        }
    }
    Err(SqlError::syntax(
        "GRAPH_TABLE requires a COLUMNS clause".into(),
    ))
}

/// `[KEEP selector] [WHERE predicate]` between the path and `COLUMNS`.
fn parse_head(head: &str) -> Result<(Option<GraphSelector>, Option<Expr>), SqlError> {
    let head = lexical::trim_separators(head);
    if head.is_empty() {
        return Ok((None, None));
    }
    let (selector, predicate) = if let Some(end) = lexical::pattern_end(head, 0, "keep") {
        let body = &head[end..];
        match lexical::find_top_level_span(body, 0, "where") {
            Some((start, after)) => (Some(parse_selector(&body[..start])?), Some(&body[after..])),
            None => (Some(parse_selector(body)?), None),
        }
    } else if let Some(end) = lexical::pattern_end(head, 0, "where") {
        (None, Some(&head[end..]))
    } else {
        return Err(unexpected(head, 0, "KEEP, WHERE or COLUMNS"));
    };
    let filter = predicate
        .map(|text| {
            let text = lexical::trim_separators(text);
            if text.is_empty() {
                Err(SqlError::syntax(
                    "GRAPH_TABLE WHERE requires a predicate".into(),
                ))
            } else {
                parse_expression(text)
            }
        })
        .transpose()?;
    Ok((selector, filter))
}

fn parse_mode(word: &str) -> Option<GraphPathMode> {
    match word {
        "walk" => Some(GraphPathMode::Walk),
        "trail" => Some(GraphPathMode::Trail),
        "acyclic" => Some(GraphPathMode::Acyclic),
        "simple" => Some(GraphPathMode::Simple),
        _ => None,
    }
}

fn parse_selector_count(word: Option<&str>) -> Result<u32, SqlError> {
    let Some(word) = word else {
        return Err(SqlError::syntax("SHORTEST requires a path count k".into()));
    };
    if word.starts_with('$') {
        return Err(SqlError::unsupported(
            "GRAPH_TABLE selector counts cannot be parameters".into(),
        ));
    }
    if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SqlError::syntax(format!(
            "SHORTEST expects an unsigned integer count, found \"{word}\""
        )));
    }
    let count = word.parse::<u32>().map_err(|_| {
        SqlError::invalid_parameter_value(format!("SHORTEST count \"{word}\" is out of range"))
    })?;
    if count == 0 {
        return Err(SqlError::invalid_parameter_value(
            "SHORTEST count must be greater than zero".into(),
        ));
    }
    Ok(count)
}

/// The text after `KEEP`: `ALL [mode] PATHS`, `ANY SHORTEST [mode] PATH` or
/// `SHORTEST k [mode] PATHS`.
fn parse_selector(text: &str) -> Result<GraphSelector, SqlError> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mut words = words.iter().map(String::as_str).peekable();
    let (kind, terminal) = match words.next() {
        Some("all") => (GraphSelectorKind::All, "paths"),
        Some("any") if words.next() == Some("shortest") => (GraphSelectorKind::AnyShortest, "path"),
        Some("shortest") => (
            GraphSelectorKind::Shortest(parse_selector_count(words.next())?),
            "paths",
        ),
        _ => {
            return Err(SqlError::syntax(
                "KEEP expects ALL, ANY SHORTEST or SHORTEST k".into(),
            ));
        }
    };
    let mode = words.peek().copied().and_then(parse_mode);
    if mode.is_some() {
        words.next();
    }
    if words.next() != Some(terminal) || words.next().is_some() {
        return Err(SqlError::syntax(format!(
            "KEEP selector expects [mode] {} at its end",
            terminal.to_ascii_uppercase()
        )));
    }
    Ok(GraphSelector {
        kind,
        mode: mode.unwrap_or_default(),
    })
}

/// `COLUMNS (projection {, projection})`, which must end the body.
fn parse_columns(after_keyword: &str) -> Result<Vec<GraphColumn>, SqlError> {
    let open = skip(after_keyword, 0);
    let close = lexical::matching_paren(after_keyword, open).ok_or_else(|| {
        SqlError::syntax("GRAPH_TABLE COLUMNS requires a parenthesized projection list".into())
    })?;
    let trailing = lexical::trim_separators(&after_keyword[close + 1..]);
    if !trailing.is_empty() {
        return Err(unexpected(trailing, 0, "the end of GRAPH_TABLE"));
    }
    split_csv(&after_keyword[open + 1..close])
        .into_iter()
        .map(parse_column)
        .collect()
}

fn parse_column(item: &str) -> Result<GraphColumn, SqlError> {
    let item = lexical::trim_separators(item);
    let mut from = 0;
    let mut alias_span = None;
    while let Some((start, end)) = lexical::find_top_level_span(item, from, "as") {
        alias_span = Some((start, end));
        from = end;
    }
    let (expression, alias) = match alias_span {
        Some((start, end)) => (&item[..start], Some(item[end..].trim())),
        None => (item, None),
    };
    let expression = lexical::trim_separators(expression);
    if expression.is_empty() {
        return Err(SqlError::syntax(
            "GRAPH_TABLE COLUMNS projection requires an expression".into(),
        ));
    }
    let alias = match alias {
        Some("") => {
            return Err(SqlError::syntax(
                "GRAPH_TABLE COLUMNS alias requires a name after AS".into(),
            ));
        }
        Some(alias) => Some(normalize_column_identifier(alias)?),
        None => None,
    };
    Ok(GraphColumn {
        expr: parse_expression(expression)?,
        alias,
    })
}

#[cfg(test)]
#[path = "graph_table_tests.rs"]
mod tests;
