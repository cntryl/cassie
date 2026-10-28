use super::{find_top_level_keyword, ParsedStatement, QueryStatement, SqlError};
use crate::sql::ast::{
    CreateMaterializedViewStatement, DropMaterializedViewStatement,
    RefreshMaterializedViewStatement,
};

pub(super) fn parse_create_materialized_view_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let prefix = "CREATE MATERIALIZED VIEW";
    let mut rest = trimmed[prefix.len()..].trim();
    let mut if_not_exists = false;
    if rest.to_ascii_lowercase().starts_with("if not exists ") {
        if_not_exists = true;
        rest = rest["IF NOT EXISTS".len()..].trim();
    }

    let as_position = find_top_level_keyword(rest, 0, "as")
        .ok_or_else(|| SqlError::syntax("CREATE MATERIALIZED VIEW requires AS".into()))?;
    let relation_definition = rest[..as_position].trim();
    if find_top_level_keyword(relation_definition, 0, "using").is_some() {
        return Err(SqlError::unsupported(
            "CREATE MATERIALIZED VIEW access methods are not supported".into(),
        ));
    }
    let (name, columns) = parse_materialized_view_name(relation_definition)?;
    let (query, with_data) = parse_population_suffix(rest[as_position + 2..].trim());
    if query.is_empty() {
        return Err(SqlError::syntax(
            "CREATE MATERIALIZED VIEW requires a query body".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateMaterializedView(CreateMaterializedViewStatement {
            name,
            if_not_exists,
            columns,
            query: query.to_string(),
            with_data,
        }),
    })
}

fn parse_materialized_view_name(raw: &str) -> Result<(String, Vec<String>), SqlError> {
    let Some(open) = raw.find('(') else {
        if parse_relation_identifier(raw).is_err() {
            return Err(SqlError::syntax(
                "CREATE MATERIALIZED VIEW supports one relation name".into(),
            ));
        }
        return Ok((parse_relation_identifier(raw)?.into_string(), Vec::new()));
    };

    let close = raw
        .rfind(')')
        .filter(|close| *close > open && raw[close + 1..].trim().is_empty())
        .ok_or_else(|| SqlError::syntax("invalid materialized-view column list".into()))?;
    let name = raw[..open].trim();
    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(SqlError::syntax(
            "CREATE MATERIALIZED VIEW supports one relation name".into(),
        ));
    }

    let columns = raw[open + 1..close]
        .split(',')
        .map(|column| column.trim().trim_matches('"').to_string())
        .collect::<Vec<_>>();
    if columns.is_empty()
        || columns
            .iter()
            .any(|column| column.is_empty() || column.split_whitespace().count() != 1)
    {
        return Err(SqlError::syntax(
            "invalid materialized-view column name".into(),
        ));
    }
    Ok((name.to_string(), columns))
}

fn parse_population_suffix(raw: &str) -> (&str, bool) {
    let lower = raw.to_ascii_lowercase();
    if lower.ends_with(" with no data") {
        (&raw[..raw.len() - " with no data".len()], false)
    } else if lower.ends_with(" with data") {
        (&raw[..raw.len() - " with data".len()], true)
    } else {
        (raw, true)
    }
}

pub(super) fn parse_refresh_materialized_view_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let prefix = "REFRESH MATERIALIZED VIEW";
    let mut rest = trimmed[prefix.len()..].trim();
    let lower = rest.to_ascii_lowercase();
    let concurrent_end = super::lexical::pattern_end(&lower, 0, "concurrently");
    let concurrently = concurrent_end.is_some();
    if let Some(concurrent_end) = concurrent_end {
        let name_start = super::lexical::separator_end(rest, concurrent_end);
        rest = rest[name_start..].trim();
    }
    let lower = rest.to_ascii_lowercase();
    let (name, with_data) = if lower.ends_with(" with no data") {
        (rest[..rest.len() - " with no data".len()].trim(), false)
    } else if lower.ends_with(" with data") {
        (rest[..rest.len() - " with data".len()].trim(), true)
    } else {
        (rest, true)
    };
    if concurrently && !with_data {
        return Err(SqlError::syntax(
            "CONCURRENTLY and WITH NO DATA options cannot be used together".into(),
        ));
    }
    let name = parse_relation_identifier(name)?.into_string();

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::RefreshMaterializedView(RefreshMaterializedViewStatement {
            name,
            with_data,
        }),
    })
}

pub(crate) fn is_concurrent_refresh(sql: &str) -> bool {
    let Ok(decoded) = super::lexical::decode_unicode_identifiers(sql) else {
        return false;
    };
    let lower = decoded.to_ascii_lowercase();
    let start = super::lexical::separator_end(&lower, 0);
    let Some(statement_end) =
        super::lexical::pattern_end(&lower, start, "refresh materialized view")
    else {
        return false;
    };
    let concurrently_start = super::lexical::separator_end(&lower, statement_end);
    super::lexical::pattern_end(&lower, concurrently_start, "concurrently").is_some()
}

pub(super) fn parse_drop_materialized_view_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let prefix = "DROP MATERIALIZED VIEW";
    let mut rest = trimmed[prefix.len()..].trim();
    let mut if_exists = false;
    if rest.to_ascii_lowercase().starts_with("if exists ") {
        if_exists = true;
        rest = rest["IF EXISTS".len()..].trim();
    }
    let rest = parse_relation_identifier(rest)?.into_string();

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropMaterializedView(DropMaterializedViewStatement {
            name: rest,
            if_exists,
        }),
    })
}

pub(super) fn parse_rename_materialized_view_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let prefix_end = super::lexical::pattern_end(trimmed, 0, "alter materialized view")
        .ok_or_else(|| SqlError::syntax("expected ALTER MATERIALIZED VIEW".into()))?;
    let rest = trimmed[prefix_end..].trim();
    let (rename, rename_end) = super::lexical::find_top_level_span(rest, 0, "rename to")
        .ok_or_else(|| {
            SqlError::unsupported("unsupported ALTER MATERIALIZED VIEW operation".into())
        })?;
    let name = parse_relation_identifier(rest[..rename].trim())?;
    let new_name = parse_relation_identifier(rest[rename_end..].trim())?;
    if !matches!(
        new_name.parsed_name().map_err(SqlError::syntax)?,
        crate::catalog::ParsedName::Unqualified(_)
    ) {
        return Err(SqlError::syntax(
            "RENAME TO requires an unqualified new name".into(),
        ));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::RenameMaterializedView(
            crate::sql::ast::RenameMaterializedViewStatement {
                name: name.into_string(),
                new_name: new_name.into_string(),
            },
        ),
    })
}

fn parse_relation_identifier(raw: &str) -> Result<crate::sql::ast::IdentifierPath, SqlError> {
    let raw = super::lexical::trim_separators(raw);
    let mut characters = raw.chars().peekable();
    let mut quoted = false;
    let mut previous = None;
    let mut separated = false;
    while let Some(character) = characters.next() {
        if quoted {
            if character == '"' {
                if characters.peek() == Some(&'"') {
                    characters.next();
                } else {
                    quoted = false;
                    previous = Some('"');
                }
            }
        } else if character.is_whitespace() {
            separated = true;
        } else {
            if separated && previous.is_some_and(|previous| previous != '.') && character != '.' {
                return Err(SqlError::syntax(
                    "unexpected tokens after relation identifier".into(),
                ));
            }
            quoted = character == '"';
            if !quoted && character != '.' && !super::lexical::identifier_character(character) {
                return Err(SqlError::syntax("invalid relation identifier".into()));
            }
            previous = Some(character);
            separated = false;
        }
    }
    let path = crate::sql::ast::IdentifierPath::parse(raw).map_err(SqlError::syntax)?;
    for component in path.components() {
        if !component.is_delimited()
            && !component
                .value()
                .chars()
                .enumerate()
                .all(|(index, character)| {
                    character == '_'
                        || character.is_alphabetic()
                        || index > 0 && (character.is_numeric() || character == '$')
                })
        {
            return Err(SqlError::syntax("expected one relation identifier".into()));
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use crate::sql::{ast::QueryStatement, parse_statement};

    #[test]
    fn should_parse_materialized_view_population_with_named_columns() {
        // Arrange
        let sql = "CREATE MATERIALIZED VIEW public.records_copy (item_id, item_name) AS \
                   SELECT id, name FROM public.records WITH NO DATA";

        // Act
        let parsed = parse_statement(sql).expect("parse materialized-view DDL");

        // Assert
        let QueryStatement::CreateMaterializedView(statement) = parsed.statement else {
            panic!("expected CREATE MATERIALIZED VIEW");
        };
        assert_eq!(statement.name, "public.records_copy");
        assert_eq!(statement.columns, ["item_id", "item_name"]);
        assert!(!statement.with_data);
        assert_eq!(statement.query, "SELECT id, name FROM public.records");
    }

    #[test]
    fn should_parse_materialized_view_refresh_with_data() {
        // Arrange
        let sql = "REFRESH MATERIALIZED VIEW public.records_copy WITH DATA";

        // Act
        let parsed = parse_statement(sql).expect("parse materialized-view refresh");

        // Assert
        assert!(matches!(
            parsed.statement,
            QueryStatement::RefreshMaterializedView(statement) if statement.with_data
        ));
    }

    #[test]
    fn should_parse_materialized_view_drop_if_exists() {
        // Arrange
        let sql = "DROP MATERIALIZED VIEW IF EXISTS public.records_copy";

        // Act
        let parsed = parse_statement(sql).expect("parse materialized-view drop");

        // Assert
        assert!(matches!(
            parsed.statement,
            QueryStatement::DropMaterializedView(statement) if statement.if_exists
        ));
    }

    #[test]
    fn should_parse_concurrent_materialized_view_refresh_without_expanding_the_ast() {
        // Arrange
        let sql = "REFRESH MATERIALIZED VIEW CONCURRENTLY public.records_copy";

        // Act
        let parsed = parse_statement(sql).expect("parse concurrent materialized-view refresh");

        // Assert
        assert!(super::is_concurrent_refresh(&parsed.raw_sql));
        assert!(matches!(
            parsed.statement,
            QueryStatement::RefreshMaterializedView(statement)
                if statement.name == "public.records_copy" && statement.with_data
        ));
    }

    #[test]
    fn should_reject_concurrent_materialized_view_refresh_with_no_data() {
        // Arrange
        let sql = "REFRESH MATERIALIZED VIEW CONCURRENTLY public.records_copy WITH NO DATA";

        // Act
        let error = parse_statement(sql).expect_err("reject CONCURRENTLY with WITH NO DATA");

        // Assert
        assert_eq!(error.kind(), crate::sql::parser::SqlErrorKind::Syntax);
        assert!(error
            .message()
            .contains("CONCURRENTLY and WITH NO DATA options cannot be used together"));
    }

    #[test]
    fn should_accept_using_as_a_materialized_view_output_column_name() {
        // Arrange
        let sql = "CREATE MATERIALIZED VIEW compat_using_column (using) AS SELECT 17 AS value";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(
            parsed.is_ok(),
            "USING inside the output-column list is not an access-method clause"
        );
    }
    #[test]
    fn should_parse_materialized_rename_with_quoted_identifiers() {
        // Arrange
        let sql = "ALTER MATERIALIZED VIEW public.\"Before Name\" RENAME TO \"After Name\"";
        // Act
        let parsed = parse_statement(sql).unwrap();
        // Assert
        assert!(
            matches!(parsed.statement, QueryStatement::RenameMaterializedView(statement)
            if statement.name == "public.\"Before Name\"" && statement.new_name == "\"After Name\"")
        );
    }

    #[test]
    fn should_reject_invalid_materialized_rename_targets() {
        // Arrange
        let inputs = [
            "ALTER MATERIALIZED VIEW source RENAME TO target garbage",
            "ALTER MATERIALIZED VIEW source extra RENAME TO target",
            "ALTER MATERIALIZED VIEW source RENAME TO other.target",
            "ALTER MATERIALIZED VIEW source RENAME TO target!",
        ];
        // Act
        let parsed = inputs.map(parse_statement);
        // Assert
        assert!(parsed.iter().all(Result::is_err));
    }
    #[test]
    fn should_parse_materialized_rename_with_keyword_separators() {
        // Arrange
        let sql = "ALTER\nMATERIALIZED VIEW source RENAME /* separator */ TO target";
        // Act
        let parsed = parse_statement(sql).unwrap();
        // Assert
        assert!(
            matches!(parsed.statement, QueryStatement::RenameMaterializedView(statement) if statement.new_name == "target")
        );
    }

    #[test]
    fn should_reject_trailing_tokens_after_truncated_materialized_names() {
        // Arrange
        let sql = format!(
            "ALTER MATERIALIZED VIEW source RENAME TO {} garbage",
            "a".repeat(80)
        );
        // Act
        let parsed = parse_statement(&sql);
        // Assert
        assert!(parsed.is_err());
    }
}
