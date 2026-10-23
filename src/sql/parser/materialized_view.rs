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
        if raw.is_empty() || raw.split_whitespace().count() != 1 {
            return Err(SqlError::syntax(
                "CREATE MATERIALIZED VIEW supports one relation name".into(),
            ));
        }
        return Ok((raw.to_string(), Vec::new()));
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
    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(SqlError::syntax(
            "REFRESH MATERIALIZED VIEW requires one relation name".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::RefreshMaterializedView(RefreshMaterializedViewStatement {
            name: name.to_string(),
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
    if rest.is_empty() || rest.split_whitespace().count() != 1 {
        return Err(SqlError::syntax(
            "DROP MATERIALIZED VIEW requires one relation name".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropMaterializedView(DropMaterializedViewStatement {
            name: rest.to_string(),
            if_exists,
        }),
    })
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
}
