//! `JSON_TABLE(context, path [AS name] [PASSING ...] COLUMNS (...) [ON ERROR])` FROM sources.

use super::super::identifiers::normalize_column_identifier;
use super::super::lexical;
use super::json_table_columns::parse_columns;
use super::{parse_expression, relation_alias, split_csv, QuerySource, SqlError};
use crate::sql::ast::{Expr, FunctionCall, JsonTableRootError, JsonTableSpec, TableFunctionForm};
use crate::sql::table_sources;

/// The `COLUMNS` keyword, skipping a qualified column such as `t.columns`.
fn columns_keyword(inner: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    loop {
        let (start, end) = lexical::find_top_level_span(inner, from, "columns")?;
        if inner[..start].trim_end().ends_with('.') {
            from = end;
        } else {
            return Some((start, end));
        }
    }
}

/// Parses `json_table(...)` followed by an optional alias. `open` and `close` delimit the
/// call's parentheses in `raw`.
pub(super) fn parse(raw: &str, open: usize, close: usize) -> Result<QuerySource, SqlError> {
    let inner = &raw[open + 1..close];
    let (head, after_columns) = columns_keyword(inner)
        .map(|(start, end)| (&inner[..start], &inner[end..]))
        .ok_or_else(|| SqlError::syntax("JSON_TABLE requires a COLUMNS clause".into()))?;
    let (context, rest) = split_context(head)?;
    let (path, path_name, passing) = parse_row_pattern(rest)?;
    let after_columns = lexical::trim_separators(after_columns);
    let columns_close = lexical::matching_paren(after_columns, 0)
        .ok_or_else(|| SqlError::syntax("JSON_TABLE COLUMNS requires a column list".into()))?;
    let columns = parse_columns(&after_columns[1..columns_close])?;
    let on_error = parse_on_error(&after_columns[columns_close + 1..])?;
    let mut args = vec![context];
    let mut names = Vec::with_capacity(passing.len());
    for (value, name) in passing {
        args.push(value);
        names.push(name);
    }
    let spec = JsonTableSpec {
        path,
        path_name,
        passing: names,
        columns,
        on_error,
    };
    let declared = spec
        .output_columns()
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    let function = FunctionCall {
        name: "json_table".into(),
        args,
        ..FunctionCall::default()
    };
    let source = QuerySource::TableFunction {
        name: function.name.clone(),
        function,
        lateral: true,
        with_ordinality: false,
        form: Some(Box::new(TableFunctionForm::JsonTable(Box::new(spec)))),
    };
    let tail = &raw[close + 1..];
    // `JSON_TABLE` has its own `FOR ORDINALITY` column; PostgreSQL's grammar has no
    // `WITH ORDINALITY` after it.
    if lexical::pattern_end(lexical::trim_separators(tail), 0, "with ordinality").is_some() {
        return Err(SqlError::syntax("syntax error at or near \"WITH\"".into()));
    }
    alias_with_declared_columns(source, tail, &declared)
}

/// Wraps `source` in its alias, naming the columns the alias list does not cover.
pub(super) fn alias_with_declared_columns(
    source: QuerySource,
    tail: &str,
    declared: &[String],
) -> Result<QuerySource, SqlError> {
    // `AS (a int)` names no alias: the function name applies.
    let tail = if tail.trim().eq_ignore_ascii_case("as") {
        ""
    } else {
        tail
    };
    let default_alias = match &source {
        QuerySource::TableFunction { function, .. } => table_sources::record_source(&function.name)
            .map_or_else(
                || function.name.clone(),
                crate::sql::table_sources::RecordSource::function_name,
            ),
        _ => String::new(),
    };
    let ordinality = matches!(
        &source,
        QuerySource::TableFunction {
            with_ordinality: true,
            ..
        }
    );
    let (source, alias, mut column_aliases) = match relation_alias::parse(source, tail)? {
        QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => (source, alias, column_aliases),
        source => (Box::new(source), default_alias, Vec::new()),
    };
    for index in column_aliases.len()..declared.len() + usize::from(ordinality) {
        column_aliases.push(
            declared
                .get(index)
                .cloned()
                .unwrap_or_else(|| "ordinality".to_owned()),
        );
    }
    Ok(QuerySource::Aliased {
        source,
        alias,
        column_aliases,
    })
}

/// Splits `context [FORMAT JSON [ENCODING UTF8]], rest` at the first top-level comma.
fn split_context(head: &str) -> Result<(Expr, &str), SqlError> {
    let (comma, comma_end) = lexical::find_top_level_span(head, 0, ",")
        .ok_or_else(|| SqlError::syntax("JSON_TABLE requires a path expression".into()))?;
    let mut context = lexical::trim_separators(&head[..comma]);
    if let Some((start, _)) = lexical::find_top_level_span(context, 0, "format json") {
        context = lexical::trim_separators(&context[..start]);
    }
    Ok((parse_expression(context)?, &head[comma_end..]))
}

type Passing = Vec<(Expr, String)>;

/// Parses `'path' [AS name] [PASSING expr AS name, ...]`.
fn parse_row_pattern(rest: &str) -> Result<(String, Option<String>, Passing), SqlError> {
    let (pattern, passing_text) = match lexical::find_top_level_span(rest, 0, "passing") {
        Some((start, end)) => (&rest[..start], Some(&rest[end..])),
        None => (rest, None),
    };
    let pattern = lexical::trim_separators(pattern);
    if !pattern.starts_with('\'') {
        return Err(SqlError::unsupported(
            "only string constants are supported in JSON_TABLE path specification".into(),
        ));
    }
    let literal_end = lexical::quoted_end(pattern, 0)
        .ok_or_else(|| SqlError::syntax("unterminated JSON_TABLE path literal".into()))?;
    let Expr::StringLiteral(path) = parse_expression(&pattern[..literal_end])? else {
        return Err(SqlError::syntax("invalid JSON_TABLE path literal".into()));
    };
    let remainder = lexical::trim_separators(&pattern[literal_end..]);
    let path_name = if remainder.is_empty() {
        None
    } else {
        let end = lexical::pattern_end(remainder, 0, "as")
            .ok_or_else(|| SqlError::syntax("syntax error in JSON_TABLE path".into()))?;
        Some(normalize_column_identifier(&remainder[end..])?)
    };
    let mut passing = Vec::new();
    for item in passing_text.map_or_default(split_csv) {
        let (start, end) = last_as(item)
            .ok_or_else(|| SqlError::syntax("JSON_TABLE PASSING requires AS name".into()))?;
        passing.push((
            parse_expression(&item[..start])?,
            normalize_column_identifier(&item[end..])?,
        ));
    }
    Ok((path, path_name, passing))
}

/// The last top-level `AS` keyword of `item`.
fn last_as(item: &str) -> Option<(usize, usize)> {
    let mut found = None;
    let mut cursor = 0;
    while let Some((start, end)) = lexical::find_top_level_span(item, cursor, "as") {
        found = Some((start, end));
        cursor = end;
    }
    found
}

/// The optional top-level `{ERROR | EMPTY [ARRAY]} ON ERROR` clause.
fn parse_on_error(tail: &str) -> Result<JsonTableRootError, SqlError> {
    let tail = lexical::trim_separators(tail);
    if tail.is_empty() {
        return Ok(JsonTableRootError::EmptyArray);
    }
    let behavior = if let Some(end) = lexical::pattern_end(tail, 0, "error on error") {
        (JsonTableRootError::Error, end)
    } else if let Some(end) = lexical::pattern_end(tail, 0, "empty array on error")
        .or_else(|| lexical::pattern_end(tail, 0, "empty on error"))
    {
        (JsonTableRootError::EmptyArray, end)
    } else {
        return Err(SqlError::syntax(
            "invalid ON ERROR behavior: only EMPTY [ ARRAY ] or ERROR is allowed in the \
             top-level ON ERROR clause"
                .into(),
        ));
    };
    if !lexical::trim_separators(&tail[behavior.1..]).is_empty() {
        return Err(SqlError::syntax("syntax error after JSON_TABLE".into()));
    }
    Ok(behavior.0)
}
