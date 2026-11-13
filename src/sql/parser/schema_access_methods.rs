/// Maps a table access method to its authoritative storage mode.
///
/// `heap` selects row-authoritative storage and `columnar` selects
/// column-authoritative storage; every other method is unsupported.
pub(super) fn parse_table_access_method(
    raw_method: &str,
    command: &str,
) -> Result<crate::catalog::CollectionStorageMode, super::SqlError> {
    let method = super::schema_identifiers::parse_identifier(raw_method.trim())?;
    match method.as_str() {
        "heap" => Ok(crate::catalog::CollectionStorageMode::RowStore),
        "columnar" => Ok(crate::catalog::CollectionStorageMode::ColumnStore),
        _ => Err(super::SqlError::unsupported(format!(
            "unsupported {command} access method '{method}'"
        ))),
    }
}

/// Canonical SQL spelling of the access method stored in `ALTER TABLE`.
pub(super) fn parse_alter_table_access_method(raw_method: &str) -> Result<String, super::SqlError> {
    let mode = parse_table_access_method(raw_method, "ALTER TABLE")?;
    Ok(if mode.uses_column_store_storage() {
        "columnar".to_string()
    } else {
        "heap".to_string()
    })
}

/// Combines a `USING` selector with a `WITH (storage = ...)` selector.
///
/// Equivalent selectors agree; conflicting selectors are a syntax error.
pub(super) fn reconcile_storage_selectors(
    using: Option<crate::catalog::CollectionStorageMode>,
    with_storage: Option<crate::catalog::CollectionStorageMode>,
) -> Result<crate::catalog::CollectionStorageMode, super::SqlError> {
    match (using, with_storage) {
        (Some(using), Some(with_storage)) if using != with_storage => {
            Err(super::SqlError::new(format!(
                "conflicting CREATE TABLE storage selectors: USING selects '{}' but WITH (storage) selects '{}'",
                using.as_str(),
                with_storage.as_str()
            )))
        }
        (Some(mode), _) | (None, Some(mode)) => Ok(mode),
        (None, None) => Ok(crate::catalog::CollectionStorageMode::RowStore),
    }
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

pub(super) fn strip_table_access_method_clause<'a>(
    raw: &'a str,
    command: &str,
) -> Result<(&'a str, Option<crate::catalog::CollectionStorageMode>), super::SqlError> {
    let raw = raw.trim();
    if !super::starts_with_keyword(raw, "using") {
        return Ok((raw, None));
    }

    let clause = raw["using".len()..].trim_start();
    let (method, trailing) = take_access_method_identifier(clause)?;
    let mode = parse_table_access_method(method, command)?;
    Ok((trailing.trim_start(), Some(mode)))
}

pub(super) fn strip_create_table_target_access_method<'a>(
    raw: &'a str,
    command: &str,
) -> Result<(&'a str, Option<crate::catalog::CollectionStorageMode>), super::SqlError> {
    let Some(position) = super::find_top_level_keyword(raw, 0, "using") else {
        return Ok((raw.trim(), None));
    };
    let clause = raw[position + "using".len()..].trim_start();
    let (method, trailing) = take_access_method_identifier(clause)?;
    let mode = parse_table_access_method(method, command)?;
    if !trailing.trim().is_empty() {
        return Err(super::SqlError::new(format!(
            "unexpected tokens after {command} access method"
        )));
    }
    Ok((raw[..position].trim_end(), Some(mode)))
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

    fn created_storage_mode(sql: &str) -> crate::catalog::CollectionStorageMode {
        let parsed = parser::parse_statement(sql).expect("parse CREATE TABLE");
        let crate::sql::QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected a create-table statement");
        };
        statement.storage_mode
    }

    #[test]
    fn should_parse_create_table_using_columnar_as_column_store() {
        // Arrange
        let sql = "CREATE TABLE columnar_table (id INT) USING columnar";

        // Act
        let mode = created_storage_mode(sql);

        // Assert
        assert_eq!(mode, crate::catalog::CollectionStorageMode::ColumnStore);
    }

    #[test]
    fn should_accept_equivalent_using_and_with_storage_selectors() {
        // Arrange
        let columnar = "CREATE TABLE t (id INT) USING columnar WITH (storage = column_store)";
        let heap = "CREATE TABLE t (id INT) USING heap WITH (storage = row_store)";

        // Act
        let columnar_mode = created_storage_mode(columnar);
        let heap_mode = created_storage_mode(heap);

        // Assert
        assert_eq!(
            columnar_mode,
            crate::catalog::CollectionStorageMode::ColumnStore
        );
        assert_eq!(heap_mode, crate::catalog::CollectionStorageMode::RowStore);
    }

    #[test]
    fn should_reject_conflicting_using_and_with_storage_selectors() {
        // Arrange
        let statements = [
            "CREATE TABLE t (id INT) USING columnar WITH (storage = row_store)",
            "CREATE TABLE t (id INT) USING heap WITH (storage = column_store)",
        ];

        for sql in statements {
            // Act
            let error = parser::parse_statement(sql).expect_err("conflicting selectors");

            // Assert
            assert!(
                error.message.contains("conflicting CREATE TABLE storage"),
                "unexpected parser error: {}",
                error.message
            );
            assert_eq!(error.kind(), crate::sql::SqlErrorKind::Syntax);
        }
    }

    #[test]
    fn should_parse_create_table_as_using_columnar() {
        // Arrange
        let sql = "CREATE TABLE copied USING columnar AS VALUES (1)";

        // Act
        let mode = created_storage_mode(sql);

        // Assert
        assert_eq!(mode, crate::catalog::CollectionStorageMode::ColumnStore);
    }

    #[test]
    fn should_reject_unsupported_create_table_access_methods() {
        // Arrange
        let sql = "CREATE TABLE brin_table (id INT) USING brin";

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
    fn should_parse_alter_table_set_access_method_columnar() {
        // Arrange
        let sql = "ALTER TABLE heap_table SET ACCESS METHOD columnar";

        // Act
        let parsed = parser::parse_statement(sql).expect("parse columnar conversion");

        // Assert
        let crate::sql::QueryStatement::AlterTable(statement) = parsed.statement else {
            panic!("expected an alter-table statement");
        };
        assert!(matches!(
            statement.operation,
            crate::sql::ast::AlterTableOperation::SetAccessMethod { ref access_method }
                if access_method == "columnar"
        ));
    }

    #[test]
    fn should_reject_unsupported_alter_table_access_methods() {
        // Arrange
        let sql = "ALTER TABLE heap_table SET ACCESS METHOD brin";

        // Act
        let error = parser::parse_statement(sql).expect_err("brin is not supported");

        // Assert
        assert!(
            error
                .message
                .contains("unsupported ALTER TABLE access method"),
            "unexpected parser error: {}",
            error.message
        );
    }

    #[test]
    fn should_reject_column_not_without_null() {
        // Arrange
        let sql = "CREATE TABLE invalid_not_clause (value TEXT NOT TRUE)";

        // Act
        let error = parser::parse_statement(sql).expect_err("NOT must be followed by NULL");

        // Assert
        assert!(
            error.message.contains("unsupported constraint 'NOT TRUE'"),
            "unexpected parser error: {}",
            error.message
        );
    }
}
