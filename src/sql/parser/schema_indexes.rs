use super::super::expr::{parse_expression, split_csv};
use super::schema_identifiers::parse_identifier;
use super::{
    find_matching_paren, find_top_level_keyword, CreateIndexStatement, DropIndexStatement, Expr,
    IndexKind, ParsedStatement, QueryStatement, SqlError,
};

use super::{parse_if_exists, parse_if_not_exists, starts_with_keyword};

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

    let name = parse_identifier(remainder[..on_pos].trim())?;
    if name.is_empty() {
        return Err(SqlError::new("CREATE INDEX missing index name".to_string()));
    }

    let remainder = remainder[on_pos + 2..].trim();
    let (table, remainder) = parse_index_target(remainder)?;
    let (kind, remainder) = parse_index_kind(remainder)?;
    let (fields, expressions, remainder) = parse_index_fields(remainder)?;
    let (include_fields, remainder) = parse_index_include_fields(remainder)?;
    let (predicate, remainder) = parse_index_predicate(remainder)?;
    let (options, remainder) = parse_index_options(remainder)?;

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
    let on_pos = find_top_level_keyword(rest, 0, "on")
        .ok_or_else(|| SqlError::new("DROP INDEX requires 'ON' clause".to_string()))?;

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

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropIndex(DropIndexStatement {
            name: name.to_string(),
            table: table.to_string(),
            if_exists,
        }),
    })
}

pub(in crate::sql::parser) fn parse_index_target(raw: &str) -> Result<(String, &str), SqlError> {
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
    let table = parse_identifier(&raw[..split])?;
    Ok((table, raw[split..].trim_start()))
}

pub(in crate::sql::parser) fn parse_index_kind(raw: &str) -> Result<(IndexKind, &str), SqlError> {
    if !starts_with_keyword(raw, "using") {
        return Ok((IndexKind::Scalar, raw));
    }

    let remainder = raw[5..].trim_start();
    if starts_with_keyword(remainder, "btree") {
        return Ok((IndexKind::Scalar, remainder[5..].trim_start()));
    }
    if starts_with_keyword(remainder, "hash") {
        return Ok((IndexKind::Scalar, remainder[4..].trim_start()));
    }
    if starts_with_keyword(remainder, "gin") {
        return Ok((IndexKind::FullText, remainder[3..].trim_start()));
    }
    if starts_with_keyword(remainder, "fulltext") {
        return Ok((IndexKind::FullText, remainder[8..].trim_start()));
    }
    if starts_with_keyword(remainder, "vector") {
        return Ok((IndexKind::Vector, remainder[6..].trim_start()));
    }
    if starts_with_keyword(remainder, "column") {
        return Ok((IndexKind::Column, remainder[6..].trim_start()));
    }
    if starts_with_keyword(remainder, "time_series") {
        return Ok((IndexKind::TimeSeries, remainder[11..].trim_start()));
    }
    if starts_with_keyword(remainder, "timeseries") {
        return Ok((IndexKind::TimeSeries, remainder[10..].trim_start()));
    }

    Err(SqlError::new("unsupported index method".to_string()))
}

pub(in crate::sql::parser) fn parse_index_fields(
    raw: &str,
) -> Result<(Vec<String>, Vec<Expr>, &str), SqlError> {
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
    for field in split_csv(field_spec) {
        let field = field.trim();
        if field.is_empty() {
            return Err(SqlError::new(
                "CREATE INDEX field cannot be empty".to_string(),
            ));
        }
        if field.starts_with('"') || is_index_field_identifier(field) {
            fields.push(parse_identifier(field)?);
        } else {
            if field.contains(';') {
                return Err(SqlError::new(
                    "invalid expression index definition".to_string(),
                ));
            }
            expressions.push(parse_expression(field)?);
        }
    }

    Ok((fields, expressions, before))
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
