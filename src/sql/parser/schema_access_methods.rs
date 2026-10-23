pub(super) fn parse_heap_access_method(
    raw_method: &str,
    command: &str,
) -> Result<String, super::SqlError> {
    let method = super::schema_identifiers::parse_identifier(raw_method.trim())?;
    if method != "heap" {
        return Err(super::SqlError::unsupported(format!(
            "unsupported {command} access method '{method}'"
        )));
    }
    Ok(method)
}

pub(super) fn take_access_method_identifier(raw: &str) -> Result<(&str, &str), super::SqlError> {
    let raw = raw.trim_start();
    if raw.is_empty() {
        return Err(super::SqlError::new(
            "USING requires an access method".to_string(),
        ));
    }
    if let Some(body) = raw.strip_prefix('"') {
        let (_, consumed) = crate::sql::identifier_text::quoted(
            body,
            crate::sql::identifier_text::IDENTIFIER_BYTES,
        )
        .ok_or_else(|| super::SqlError::new(format!("unterminated quoted identifier '{raw}'")))?;
        let end = consumed + 1;
        return Ok((&raw[..end], &raw[end..]));
    }

    let end = raw.find(char::is_whitespace).unwrap_or(raw.len());
    Ok((&raw[..end], &raw[end..]))
}

pub(super) fn strip_heap_access_method_clause<'a>(
    raw: &'a str,
    command: &str,
) -> Result<&'a str, super::SqlError> {
    let raw = raw.trim();
    if !super::starts_with_keyword(raw, "using") {
        return Ok(raw);
    }

    let clause = raw["using".len()..].trim_start();
    let (method, trailing) = take_access_method_identifier(clause)?;
    parse_heap_access_method(method, command)?;
    Ok(trailing.trim_start())
}

pub(super) fn strip_create_table_target_access_method<'a>(
    raw: &'a str,
    command: &str,
) -> Result<&'a str, super::SqlError> {
    let Some(position) = super::find_top_level_keyword(raw, 0, "using") else {
        return Ok(raw.trim());
    };
    let clause = raw[position + "using".len()..].trim_start();
    let (method, trailing) = take_access_method_identifier(clause)?;
    parse_heap_access_method(method, command)?;
    if !trailing.trim().is_empty() {
        return Err(super::SqlError::new(format!(
            "unexpected tokens after {command} access method"
        )));
    }
    Ok(raw[..position].trim_end())
}

#[cfg(test)]
mod tests {
    use crate::sql::parser;

    #[test]
    fn should_parse_create_table_using_heap() {
        // Arrange
        let sql = "CREATE TABLE heap_table (id INT) USING heap";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        assert!(parsed.is_ok(), "CREATE TABLE USING heap: {parsed:?}");
    }

    #[test]
    fn should_parse_alter_table_set_access_method_heap() {
        // Arrange
        let sql = "ALTER TABLE heap_table SET ACCESS METHOD heap";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        assert!(
            parsed.is_ok(),
            "ALTER TABLE SET ACCESS METHOD heap: {parsed:?}"
        );
    }

    #[test]
    fn should_reject_unsupported_create_table_access_methods() {
        // Arrange
        let sql = "CREATE TABLE columnar_table (id INT) USING columnar";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        let error = parsed.expect_err("unsupported CREATE TABLE methods should fail");
        assert!(
            error
                .message
                .contains("unsupported CREATE TABLE access method"),
            "unexpected parser error: {}",
            error.message
        );
    }

    #[test]
    fn should_reject_unsupported_alter_table_access_methods() {
        // Arrange
        let sql = "ALTER TABLE heap_table SET ACCESS METHOD columnar";

        // Act
        let error = parser::parse_statement(sql).expect_err("columnar is not supported");

        // Assert
        assert!(
            error
                .message
                .contains("unsupported ALTER TABLE access method"),
            "unexpected parser error: {}",
            error.message
        );
    }
}
