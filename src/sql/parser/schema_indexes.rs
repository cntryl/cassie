use super::super::expr::{parse_expression, split_csv};
use super::schema_identifiers::{parse_column_reference, parse_identifier, parse_relation_path};
use super::{
    find_matching_paren, find_top_level_keyword, CreateIndexStatement, DropIndexStatement, Expr,
    IndexKind, ParsedStatement, QueryStatement, SqlError,
};

use super::{parse_if_exists, parse_if_not_exists, starts_with_keyword};

type ParsedIndexFields<'a> = (
    Vec<String>,
    Vec<Expr>,
    std::collections::BTreeMap<String, String>,
    &'a str,
);
type ParsedIndexKeyModifiers<'a> = (&'a str, Option<String>, Option<String>, Option<String>);

pub(in crate::sql::parser) fn parse_create_index_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_lowercase();

    let mut unique = false;
    let remainder = if lower.starts_with("create unique index ") {
        unique = true;
        &trimmed["create unique index ".len()..]
    } else if lower.starts_with("create index ") {
        &trimmed["create index ".len()..]
    } else {
        return Err(SqlError::new(
            "unsupported CREATE INDEX statement".to_string(),
        ));
    };

    if starts_with_keyword(remainder, "concurrently") {
        return Err(SqlError::new(
            "CREATE INDEX CONCURRENTLY is not supported in this version".to_string(),
        ));
    }

    let (if_not_exists, remainder) = parse_if_not_exists(remainder);

    let on_pos = find_top_level_keyword(remainder, 0, "on")
        .ok_or_else(|| SqlError::new("CREATE INDEX requires 'ON' clause".to_string()))?;

    let raw_name = remainder[..on_pos].trim();
    let name = if raw_name.is_empty() {
        String::new()
    } else {
        parse_identifier(raw_name)?
    };
    if !raw_name.is_empty() && name.is_empty() {
        return Err(SqlError::new("CREATE INDEX missing index name".to_string()));
    }

    let remainder = remainder[on_pos + 2..].trim();
    let (table, remainder) = parse_index_target(remainder)?;
    let (kind, access_method, remainder) = parse_index_kind(remainder)?;
    let (fields, expressions, key_options, remainder) = parse_index_fields(remainder)?;
    let (include_fields, remainder) = parse_index_include_fields(remainder)?;
    let (predicate, remainder) = parse_index_predicate(remainder)?;
    let (mut options, remainder) = parse_index_options(remainder)?;
    if options
        .keys()
        .any(|key| key.starts_with("__cassie_index_key_"))
    {
        return Err(SqlError::unsupported(
            "index key metadata options are reserved for SQL key clauses".to_string(),
        ));
    }
    options.extend(key_options);

    if !remainder.is_empty() {
        return Err(SqlError::new("unsupported CREATE INDEX syntax".to_string()));
    }

    if !matches!(kind, IndexKind::Scalar | IndexKind::Column)
        && fields.len() + expressions.len() > 1
    {
        return Err(SqlError::unsupported(
            "composite indexes are only supported for scalar index methods".to_string(),
        ));
    }
    if !matches!(kind, IndexKind::Scalar) && !expressions.is_empty() {
        return Err(SqlError::unsupported(
            "expression indexes are only supported for scalar index methods".to_string(),
        ));
    }
    if matches!(kind, IndexKind::Scalar) && access_method == Some("hash") {
        options.insert(
            crate::catalog::IndexMeta::ACCESS_METHOD_OPTION.to_string(),
            "hash".to_string(),
        );
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateIndex(CreateIndexStatement {
            name,
            table: table.clone(),
            fields,
            expressions,
            include_fields,
            predicate,
            if_not_exists,
            unique,
            kind,
            options,
        }),
    })
}

pub(in crate::sql::parser) fn parse_drop_index_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[10..].trim();

    let (if_exists, rest) = parse_if_exists(rest);
    if starts_with_keyword(rest, "concurrently") {
        return Err(SqlError::new(
            "DROP INDEX CONCURRENTLY is not supported".to_string(),
        ));
    }
    let (name, table) = match find_top_level_keyword(rest, 0, "on") {
        Some(on_pos) => {
            let name = rest[..on_pos].trim();
            let table = rest[on_pos + 2..].trim();
            if name.is_empty() || table.is_empty() {
                return Err(SqlError::new(
                    "DROP INDEX requires index name and table".to_string(),
                ));
            }
            if table.contains(' ') {
                return Err(SqlError::new(
                    "unsupported tokens after DROP INDEX table name".to_string(),
                ));
            }
            (name, table.to_string())
        }
        None if !rest.is_empty() => {
            crate::sql::ast::IdentifierPath::parse(rest).map_err(SqlError::new)?;
            (rest, String::new())
        }
        None => {
            return Err(SqlError::new(
                "unsupported tokens after DROP INDEX name".to_string(),
            ));
        }
    };

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropIndex(DropIndexStatement {
            name: name.to_string(),
            table,
            if_exists,
        }),
    })
}

pub(in crate::sql::parser) fn parse_index_target(
    raw: &str,
) -> Result<(crate::sql::ast::IdentifierPath, &str), SqlError> {
    let raw = raw.trim_start();
    if raw.is_empty() {
        return Err(SqlError::new(
            "missing table name in CREATE INDEX".to_string(),
        ));
    }

    let split = raw
        .char_indices()
        .find_map(|(idx, ch)| (ch.is_whitespace() || ch == '(').then_some(idx))
        .unwrap_or(raw.len());
    let table = parse_relation_path(&raw[..split])?;
    Ok((table, raw[split..].trim_start()))
}

pub(in crate::sql::parser) fn parse_index_kind(
    raw: &str,
) -> Result<(IndexKind, Option<&'static str>, &str), SqlError> {
    if !starts_with_keyword(raw, "using") {
        return Ok((IndexKind::Scalar, None, raw));
    }

    let remainder = raw[5..].trim_start();
    if starts_with_keyword(remainder, "btree") {
        return Ok((
            IndexKind::Scalar,
            Some("btree"),
            remainder[5..].trim_start(),
        ));
    }
    if starts_with_keyword(remainder, "hash") {
        return Ok((IndexKind::Scalar, Some("hash"), remainder[4..].trim_start()));
    }
    if starts_with_keyword(remainder, "gin") {
        return Ok((
            IndexKind::FullText,
            Some("gin"),
            remainder[3..].trim_start(),
        ));
    }
    if starts_with_keyword(remainder, "fulltext") {
        return Ok((
            IndexKind::FullText,
            Some("fulltext"),
            remainder[8..].trim_start(),
        ));
    }
    if starts_with_keyword(remainder, "vector") {
        return Ok((
            IndexKind::Vector,
            Some("vector"),
            remainder[6..].trim_start(),
        ));
    }
    if starts_with_keyword(remainder, "column") {
        return Ok((
            IndexKind::Column,
            Some("column"),
            remainder[6..].trim_start(),
        ));
    }
    if starts_with_keyword(remainder, "time_series") {
        return Ok((
            IndexKind::TimeSeries,
            Some("time_series"),
            remainder[11..].trim_start(),
        ));
    }
    if starts_with_keyword(remainder, "timeseries") {
        return Ok((
            IndexKind::TimeSeries,
            Some("timeseries"),
            remainder[10..].trim_start(),
        ));
    }

    Err(SqlError::new("unsupported index method".to_string()))
}

pub(in crate::sql::parser) fn parse_index_fields(
    raw: &str,
) -> Result<ParsedIndexFields<'_>, SqlError> {
    let raw = raw.trim();
    if !raw.starts_with('(') {
        return Err(SqlError::new(
            "CREATE INDEX requires indexed field list in parentheses".to_string(),
        ));
    }

    let close = find_matching_paren(raw, 0)
        .ok_or_else(|| SqlError::new("CREATE INDEX field list missing closing ')'".to_string()))?;
    let field_spec = &raw[1..close];
    if field_spec.trim().is_empty() {
        return Err(SqlError::new(
            "CREATE INDEX field cannot be empty".to_string(),
        ));
    }

    let before = raw[close + 1..].trim();
    let mut fields = Vec::new();
    let mut expressions = Vec::new();
    let mut key_options = std::collections::BTreeMap::new();
    let mut has_key_modifiers = false;
    for (position, field) in split_csv(field_spec).into_iter().enumerate() {
        let field = field.trim();
        if field.is_empty() {
            return Err(SqlError::new(
                "CREATE INDEX field cannot be empty".to_string(),
            ));
        }
        let (field, operator_class, order, nulls) = parse_index_key_modifiers(field)?;
        for (option, value) in [
            ("operator_class", operator_class),
            ("order", order),
            ("nulls", nulls),
        ] {
            if let Some(value) = value {
                has_key_modifiers = true;
                key_options.insert(
                    crate::catalog::IndexMeta::key_option_name(position, option),
                    value,
                );
            }
        }
        if field.starts_with('"') || is_index_field_identifier(field) {
            fields.push(parse_column_reference(field)?);
        } else {
            if field.contains(';') {
                return Err(SqlError::new(
                    "invalid expression index definition".to_string(),
                ));
            }
            expressions.push(parse_expression(field)?);
        }
    }

    if has_key_modifiers && !fields.is_empty() && !expressions.is_empty() {
        return Err(SqlError::unsupported(
            "ordered or operator-class index keys cannot mix field and expression keys".to_string(),
        ));
    }

    Ok((fields, expressions, key_options, before))
}

fn parse_index_key_modifiers(raw: &str) -> Result<ParsedIndexKeyModifiers<'_>, SqlError> {
    let mut field = raw.trim_end();
    let mut order = None;
    let mut nulls = None;
    if let Some(remainder) = strip_trailing_words(field, &["nulls", "first"]) {
        field = remainder;
        nulls = Some("first".to_string());
    } else if let Some(remainder) = strip_trailing_words(field, &["nulls", "last"]) {
        field = remainder;
        nulls = Some("last".to_string());
    }
    if let Some(remainder) = strip_trailing_words(field, &["asc"]) {
        field = remainder;
        order = Some("asc".to_string());
    } else if let Some(remainder) = strip_trailing_words(field, &["desc"]) {
        field = remainder;
        order = Some("desc".to_string());
    }

    let mut operator_class = None;
    if let Some((remainder, candidate)) = strip_trailing_operator_class(field) {
        field = remainder;
        operator_class = Some(
            if candidate.eq_ignore_ascii_case("pg_catalog.text_pattern_ops") {
                "text_pattern_ops".to_string()
            } else {
                candidate
            },
        );
    }
    if field.trim().is_empty() {
        return Err(SqlError::new(
            "CREATE INDEX key modifier requires an indexed field".to_string(),
        ));
    }
    Ok((field.trim_end(), operator_class, order, nulls))
}

fn strip_trailing_words<'a>(raw: &'a str, words: &[&str]) -> Option<&'a str> {
    let mut end = raw.trim_end().len();
    for expected in words.iter().rev() {
        let candidate = &raw[..end];
        let split = candidate.rfind(char::is_whitespace);
        let (prefix, last) = split.map_or(("", candidate), |split| {
            (&candidate[..split], &candidate[split + 1..])
        });
        if !last.eq_ignore_ascii_case(expected) {
            return None;
        }
        end = prefix.trim_end().len();
    }
    Some(&raw[..end])
}

fn strip_trailing_operator_class(raw: &str) -> Option<(&str, String)> {
    let trimmed = raw.trim_end();
    let split = trimmed.rfind(char::is_whitespace)?;
    let candidate = trimmed[split + 1..].trim();
    if candidate.is_empty() || !candidate.to_ascii_lowercase().ends_with("_ops") {
        return None;
    }
    let remainder = trimmed[..split].trim_end();
    (!remainder.is_empty()).then(|| (remainder, candidate.to_ascii_lowercase()))
}

pub(in crate::sql::parser) fn is_index_field_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(in crate::sql::parser) fn parse_index_include_fields(
    raw: &str,
) -> Result<(Vec<String>, &str), SqlError> {
    let raw = raw.trim();
    if raw.is_empty() || !starts_with_keyword(raw, "include") {
        return Ok((Vec::new(), raw));
    }

    let body = raw["include".len()..].trim_start();
    if !body.starts_with('(') {
        return Err(SqlError::new(
            "INCLUDE requires field list in parentheses".to_string(),
        ));
    }
    let close = body
        .find(')')
        .ok_or_else(|| SqlError::new("INCLUDE field list missing closing ')'".to_string()))?;
    let field_spec = &body[1..close];
    if field_spec.trim().is_empty() {
        return Err(SqlError::new("INCLUDE field cannot be empty".to_string()));
    }

    let mut fields = Vec::new();
    for field in split_csv(field_spec) {
        let field = field.trim();
        if field.is_empty() {
            return Err(SqlError::new("INCLUDE field cannot be empty".to_string()));
        }
        if field
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '(' | ')' | ';'))
        {
            return Err(SqlError::new(
                "expression INCLUDE definitions are not supported".to_string(),
            ));
        }
        fields.push(field.to_string());
    }

    Ok((fields, body[close + 1..].trim_start()))
}

pub(in crate::sql::parser) fn parse_index_predicate(
    raw: &str,
) -> Result<(Option<Expr>, &str), SqlError> {
    let raw = raw.trim();
    if raw.is_empty() || !starts_with_keyword(raw, "where") {
        return Ok((None, raw));
    }
    let predicate = raw["where".len()..].trim();
    if predicate.is_empty() {
        return Err(SqlError::new(
            "CREATE INDEX WHERE requires predicate".to_string(),
        ));
    }
    Ok((Some(parse_expression(predicate)?), ""))
}

pub(in crate::sql::parser) fn parse_index_options(
    raw: &str,
) -> Result<(std::collections::BTreeMap<String, String>, &str), SqlError> {
    let mut options = std::collections::BTreeMap::new();
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok((options, raw));
    }

    if !starts_with_keyword(raw, "with") {
        return Ok((options, raw));
    }

    let with_body = raw[4..].trim_start();
    if !with_body.starts_with('(') || !with_body.ends_with(')') {
        return Err(SqlError::new(
            "WITH options must be enclosed in parentheses".to_string(),
        ));
    }

    let body = &with_body[1..with_body.len() - 1];
    for token in split_csv(body) {
        let token = token.trim();
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| SqlError::new("index option must be key=value".to_string()))?;
        let key = key.trim().to_lowercase();
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if key.is_empty() {
            return Err(SqlError::new(
                "index option key cannot be empty".to_string(),
            ));
        }
        options.insert(key, value);
    }

    Ok((options, ""))
}

pub(in crate::sql::parser) fn parse_reindex_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let mut remainder = trimmed["reindex".len()..].trim();

    if remainder.starts_with('(') {
        let close = find_matching_paren(remainder, 0)
            .ok_or_else(|| SqlError::syntax("unterminated REINDEX options".to_string()))?;
        let options = &remainder[1..close];
        for option in split_csv(options) {
            let mut parts = option.split_whitespace();
            let name = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or_default();
            if parts.next().is_some() || value.is_empty() {
                return Err(SqlError::syntax(format!(
                    "invalid REINDEX option '{option}'"
                )));
            }
            match (
                name.to_ascii_lowercase().as_str(),
                value.to_ascii_lowercase().as_str(),
            ) {
                ("verbose" | "concurrently", "false") => {}
                ("concurrently", "true") => {
                    return Err(SqlError::unsupported(
                        "REINDEX CONCURRENTLY is not supported".to_string(),
                    ));
                }
                ("verbose", "true") => {
                    return Err(SqlError::unsupported(
                        "verbose REINDEX output is not supported".to_string(),
                    ));
                }
                _ => {
                    return Err(SqlError::unsupported(format!(
                        "unsupported REINDEX option '{option}'"
                    )));
                }
            }
        }
        remainder = remainder[close + 1..].trim();
    }

    let (object, raw_name) = if starts_with_keyword(remainder, "index") {
        ("index", remainder["index".len()..].trim())
    } else if starts_with_keyword(remainder, "table") {
        ("table", remainder["table".len()..].trim())
    } else {
        return Err(SqlError::unsupported(
            "REINDEX currently supports INDEX and TABLE targets".to_string(),
        ));
    };

    if starts_with_keyword(raw_name, "concurrently") {
        return Err(SqlError::unsupported(
            "REINDEX CONCURRENTLY is not supported".to_string(),
        ));
    }
    if raw_name.is_empty() {
        return Err(SqlError::syntax(format!(
            "REINDEX {object} requires a name"
        )));
    }

    let name = super::schema_identifiers::parse_identifier(raw_name)?;
    let target = if object == "index" {
        crate::sql::ast::ReindexTarget::Index {
            collection: String::new(),
            name,
        }
    } else {
        crate::sql::ast::ReindexTarget::Table { name }
    };

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Reindex(crate::sql::ast::ReindexStatement { target }),
    })
}
