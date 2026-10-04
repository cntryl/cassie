use super::SqlError;
use crate::sql::ast::IdentifierPath;
use crate::sql::ColumnIdentifierPath;

pub(super) fn parse_relation_path(raw: &str) -> Result<IdentifierPath, SqlError> {
    IdentifierPath::parse(raw).map_err(SqlError::new)
}

pub(super) fn parse_identifier(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }

    if raw.starts_with('"') {
        let Some(unquoted) = raw.strip_suffix('"') else {
            return Err(SqlError::new(format!(
                "unterminated quoted identifier '{raw}'"
            )));
        };
        if unquoted.len() < 2 {
            return Err(SqlError::new("empty quoted identifier".to_string()));
        }
        return Ok(unquoted[1..].replace("\"\"", "\""));
    }

    if raw.chars().any(char::is_whitespace) {
        return Err(SqlError::new(format!("invalid identifier '{raw}'")));
    }

    Ok(raw.to_string())
}

pub(super) fn parse_column_identifier(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let path = ColumnIdentifierPath::parse(raw).map_err(SqlError::new)?;
    if path.is_qualified() {
        return Err(SqlError::new(format!("invalid column identifier '{raw}'")));
    }
    Ok(path.declared_name())
}

pub(super) fn parse_column_reference(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let path = ColumnIdentifierPath::parse(raw).map_err(SqlError::new)?;
    if path.is_qualified() {
        return Err(SqlError::new(format!("invalid column identifier '{raw}'")));
    }
    Ok(path.field_lookup_key())
}

pub(super) fn parse_column_identifier_list(raw: &str) -> Result<Vec<String>, SqlError> {
    super::split_csv(raw)
        .into_iter()
        .map(|field| parse_column_reference(field.trim()))
        .collect()
}
