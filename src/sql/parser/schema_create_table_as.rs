use super::schema_identifiers::parse_column_identifier_list;

pub(super) fn parse_create_table_as_statement(
    raw_sql: &str,
    if_not_exists: bool,
    rest: &str,
) -> Result<Option<crate::sql::ast::ParsedStatement>, super::SqlError> {
    let Some(as_position) = super::find_top_level_keyword(rest, 0, "as") else {
        return Ok(None);
    };
    let (table, column_names) = parse_target(rest[..as_position].trim())?;
    let raw_query = rest[as_position + 2..].trim();
    let (raw_query, with_data) = strip_data_option(raw_query);
    if raw_query.is_empty() {
        return Err(super::SqlError::new(
            "CREATE TABLE AS requires a query body".to_string(),
        ));
    }

    let normalized_query = normalize_table_query(raw_query)?;
    let query = super::super::parse_statement(&normalized_query)?;
    if !matches!(
        &query.statement,
        super::QueryStatement::Select(_)
            | super::QueryStatement::SessionObject(
                crate::sql::ast::SessionObjectStatement::Execute(_)
            )
    ) {
        return Err(super::SqlError::unsupported(
            "CREATE TABLE AS requires a SELECT, TABLE, VALUES, or EXECUTE query".to_string(),
        ));
    }

    if !column_names.is_empty() {
        let mut seen = std::collections::HashSet::new();
        for name in &column_names {
            if !seen.insert(name) {
                return Err(super::SqlError::duplicate_column(name));
            }
        }
    }

    Ok(Some(crate::sql::ast::ParsedStatement {
        raw_sql: raw_sql.to_string(),
        statement: super::QueryStatement::CreateTable(crate::sql::ast::CreateTableStatement {
            table,
            fields: Vec::new(),
            if_not_exists,
            storage_mode: crate::catalog::CollectionStorageMode::RowStore,
            as_query: Some(crate::sql::ast::CreateTableAsClause {
                query: Box::new(query),
                column_names,
                with_data,
            }),
            like: None,
        }),
    }))
}

fn parse_target(
    target: &str,
) -> Result<(crate::sql::ast::IdentifierPath, Vec<String>), super::SqlError> {
    let target = super::schema_access_methods::strip_create_table_target_access_method(
        target,
        "CREATE TABLE",
    )?;

    if let Some(open) = target.find('(') {
        let close = super::find_matching_paren(target, open)
            .ok_or_else(|| super::SqlError::new("invalid CREATE TABLE AS column list".into()))?;
        if !target[close + 1..].trim().is_empty() {
            return Err(super::SqlError::new(
                "unexpected tokens after CREATE TABLE AS column list".into(),
            ));
        }
        let table = super::parse_relation_path(target[..open].trim())?;
        let column_names = parse_column_identifier_list(&target[open + 1..close])?;
        if column_names.is_empty() {
            return Err(super::SqlError::new(
                "CREATE TABLE AS column list cannot be empty".into(),
            ));
        }
        return Ok((table, column_names));
    }
    Ok((super::parse_relation_path(target)?, Vec::new()))
}

fn strip_data_option(query: &str) -> (&str, bool) {
    let lowered = query.to_ascii_lowercase();
    for (suffix, with_data) in [("with no data", false), ("with data", true)] {
        if lowered.ends_with(suffix) {
            let prefix_end = lowered.len() - suffix.len();
            if prefix_end > 0
                && query[..prefix_end]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace)
            {
                return (query[..prefix_end].trim_end(), with_data);
            }
        }
    }
    (query, true)
}

fn normalize_table_query(query: &str) -> Result<String, super::SqlError> {
    let trimmed = query.trim();
    if trimmed.len() > 5
        && trimmed
            .get(..5)
            .is_some_and(|keyword| keyword.eq_ignore_ascii_case("table"))
        && trimmed.as_bytes()[5].is_ascii_whitespace()
    {
        let source = super::parse_relation_path(trimmed[5..].trim())?;
        return Ok(format!("SELECT * FROM {source}"));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use crate::sql::{parser, QueryStatement};

    #[test]
    fn should_parse_create_table_as_with_values_and_output_names() {
        // Arrange
        let sql = "CREATE TABLE copied (copy_id, copy_label) AS VALUES (1, 'one')";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        let parsed = parsed.expect("parse CREATE TABLE AS VALUES");
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected a create-table statement");
        };
        let Some(as_query) = statement.as_query else {
            panic!("expected a typed CTAS clause");
        };
        assert_eq!(statement.table, "copied");
        assert_eq!(as_query.column_names, ["copy_id", "copy_label"]);
        assert!(as_query.with_data);
        assert!(matches!(
            as_query.query.statement,
            QueryStatement::Select(_)
        ));
    }

    #[test]
    fn should_parse_create_table_as_table_with_no_data() {
        // Arrange
        let sql = "CREATE TABLE copied AS TABLE source WITH NO DATA";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        let parsed = parsed.expect("parse CREATE TABLE AS TABLE");
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected a create-table statement");
        };
        let Some(as_query) = statement.as_query else {
            panic!("expected a typed CTAS clause");
        };
        assert!(as_query.column_names.is_empty());
        assert!(!as_query.with_data);
        assert!(matches!(
            as_query.query.statement,
            QueryStatement::Select(_)
        ));
    }

    #[test]
    fn should_parse_create_table_as_table_with_output_names() {
        // Arrange
        let sql = "CREATE TABLE copied (copy_id, copy_label) AS TABLE source";

        // Act
        let parsed = parser::parse_statement(sql).expect("parse CREATE TABLE AS TABLE");

        // Assert
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected a create-table statement");
        };
        let Some(as_query) = statement.as_query else {
            panic!("expected a typed CTAS clause");
        };
        assert_eq!(as_query.column_names, ["copy_id", "copy_label"]);
    }

    #[test]
    fn should_parse_create_table_as_with_heap_access_method() {
        // Arrange
        let sql = "CREATE TABLE copied USING heap AS VALUES (1)";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        let parsed = parsed.expect("parse CREATE TABLE AS USING heap");
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected a create-table statement");
        };
        assert_eq!(statement.table, "copied");
        assert!(statement.as_query.is_some());
    }

    #[test]
    fn should_reject_unsupported_create_table_as_access_methods() {
        // Arrange
        let sql = "CREATE TABLE copied USING columnar AS VALUES (1)";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        let error = parsed.expect_err("unsupported CTAS access methods should fail");
        assert!(error
            .message
            .contains("unsupported CREATE TABLE access method"));
    }

    #[test]
    fn should_reject_duplicate_create_table_as_output_names() {
        // Arrange
        let sql = "CREATE TABLE copied (id, id) AS VALUES (1, 2)";

        // Act
        let parsed = parser::parse_statement(sql);

        // Assert
        let error = parsed.expect_err("duplicate CTAS output names should fail");
        assert!(error
            .message
            .contains("column \"id\" specified more than once"));
    }
}
