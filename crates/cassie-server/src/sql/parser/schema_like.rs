use super::{schema_identifiers::parse_relation_path, SqlError};
use crate::sql::ast::{CreateTableLikeClause, CreateTableLikeFeature, CreateTableLikeOption};

pub(super) fn is_like_table_element(raw: &str) -> bool {
    raw.split_whitespace()
        .next()
        .is_some_and(|token| token.eq_ignore_ascii_case("LIKE"))
}

pub(super) fn parse_create_table_like_clause(raw: &str) -> Result<CreateTableLikeClause, SqlError> {
    let raw = raw.trim();
    let after_keyword = raw
        .get("LIKE".len()..)
        .filter(|_| is_like_table_element(raw))
        .ok_or_else(|| SqlError::syntax("expected LIKE table element".into()))?
        .trim_start();
    if after_keyword.is_empty() {
        return Err(SqlError::syntax(
            "CREATE TABLE LIKE requires a source table".into(),
        ));
    }

    let source_end = source_relation_end(after_keyword);
    let source = parse_relation_path(after_keyword[..source_end].trim())?;
    if source.is_empty() {
        return Err(SqlError::syntax(
            "CREATE TABLE LIKE requires a source table".into(),
        ));
    }

    let options = parse_like_options(&after_keyword[source_end..])?;
    Ok(CreateTableLikeClause { source, options })
}

fn source_relation_end(raw: &str) -> usize {
    let mut in_quotes = false;
    let mut characters = raw.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == '"' {
            if in_quotes && characters.peek().is_some_and(|(_, next)| *next == '"') {
                characters.next();
                continue;
            }
            in_quotes = !in_quotes;
        } else if !in_quotes && character.is_whitespace() {
            return index;
        }
    }
    raw.len()
}

fn parse_like_options(raw: &str) -> Result<Vec<CreateTableLikeOption>, SqlError> {
    let mut tokens = raw.split_whitespace();
    let mut options = Vec::new();

    while let Some(action) = tokens.next() {
        let feature_name = tokens.next().ok_or_else(|| {
            SqlError::syntax(format!(
                "CREATE TABLE LIKE {action} requires an option name"
            ))
        })?;
        let feature = parse_like_feature(feature_name).ok_or_else(|| {
            SqlError::unsupported(format!(
                "unsupported CREATE TABLE LIKE option '{feature_name}'"
            ))
        })?;
        let option = if action.eq_ignore_ascii_case("INCLUDING") {
            CreateTableLikeOption::Including(feature)
        } else if action.eq_ignore_ascii_case("EXCLUDING") {
            CreateTableLikeOption::Excluding(feature)
        } else {
            return Err(SqlError::unsupported(format!(
                "unsupported CREATE TABLE LIKE option action '{action}'"
            )));
        };
        options.push(option);
    }

    Ok(options)
}

fn parse_like_feature(raw: &str) -> Option<CreateTableLikeFeature> {
    if raw.eq_ignore_ascii_case("ALL") {
        Some(CreateTableLikeFeature::All)
    } else if raw.eq_ignore_ascii_case("COMMENTS") {
        Some(CreateTableLikeFeature::Comments)
    } else if raw.eq_ignore_ascii_case("COMPRESSION") {
        Some(CreateTableLikeFeature::Compression)
    } else if raw.eq_ignore_ascii_case("CONSTRAINTS") {
        Some(CreateTableLikeFeature::Constraints)
    } else if raw.eq_ignore_ascii_case("DEFAULTS") {
        Some(CreateTableLikeFeature::Defaults)
    } else if raw.eq_ignore_ascii_case("GENERATED") {
        Some(CreateTableLikeFeature::Generated)
    } else if raw.eq_ignore_ascii_case("IDENTITY") {
        Some(CreateTableLikeFeature::Identity)
    } else if raw.eq_ignore_ascii_case("INDEXES") {
        Some(CreateTableLikeFeature::Indexes)
    } else if raw.eq_ignore_ascii_case("STATISTICS") {
        Some(CreateTableLikeFeature::Statistics)
    } else if raw.eq_ignore_ascii_case("STORAGE") {
        Some(CreateTableLikeFeature::Storage)
    } else {
        None
    }
}
