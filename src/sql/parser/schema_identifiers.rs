use super::SqlError;

pub(super) fn parse_identifier(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }

    if raw.starts_with('"') {
        let Some(unquoted) = raw.strip_suffix('"') else {
            return Err(SqlError(format!("unterminated quoted identifier '{raw}'")));
        };
        if unquoted.len() < 2 {
            return Err(SqlError("empty quoted identifier".to_string()));
        }
        return Ok(unquoted[1..].replace("\"\"", "\""));
    }

    if raw.chars().any(char::is_whitespace) {
        return Err(SqlError(format!("invalid identifier '{raw}'")));
    }

    Ok(raw.to_string())
}

pub(super) fn parse_identifier_list(raw: &str) -> Result<Vec<String>, SqlError> {
    super::split_csv(raw)
        .into_iter()
        .map(|field| parse_identifier(field.trim()))
        .collect()
}
