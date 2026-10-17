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

    if let Some(body) = raw.strip_prefix('"') {
        let Some((decoded, consumed)) = crate::sql::identifier_text::quoted(
            body,
            crate::sql::identifier_text::IDENTIFIER_BYTES,
        ) else {
            return Err(SqlError::new(format!(
                "unterminated quoted identifier '{raw}'"
            )));
        };
        if decoded.is_empty() {
            return Err(SqlError::new("empty quoted identifier".to_owned()));
        }
        if consumed == body.len() {
            return Ok(decoded);
        }
        return parse_relation_path(raw).map(IdentifierPath::into_string);
    }

    if raw.chars().any(char::is_whitespace) {
        return Err(SqlError::new(format!("invalid identifier '{raw}'")));
    }

    if raw.contains('.') {
        return parse_relation_path(raw).map(IdentifierPath::into_string);
    }
    Ok(crate::sql::identifier_text::prefix(raw).to_ascii_lowercase())
}

pub(super) fn parse_column_identifier(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let path = ColumnIdentifierPath::parse_sql(raw).map_err(SqlError::new)?;
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
    let path = ColumnIdentifierPath::parse_sql(raw).map_err(SqlError::new)?;
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
