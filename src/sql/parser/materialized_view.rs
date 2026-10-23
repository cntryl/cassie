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
    let (name, columns) = parse_materialized_view_name(rest[..as_position].trim())?;
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
    let rest = trimmed[prefix.len()..].trim();
    let lower = rest.to_ascii_lowercase();
    if lower.starts_with("concurrently ") {
        return Err(SqlError::unsupported(
            "REFRESH MATERIALIZED VIEW CONCURRENTLY is not supported".into(),
        ));
    }
    let (name, with_data) = if lower.ends_with(" with no data") {
        (rest[..rest.len() - " with no data".len()].trim(), false)
    } else if lower.ends_with(" with data") {
        (rest[..rest.len() - " with data".len()].trim(), true)
    } else {
        (rest, true)
    };
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
    fn should_parse_materialized_view_population_and_columns() {
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
    fn should_parse_materialized_view_refresh_and_drop() {
        // Arrange / Act
        let refresh = parse_statement("REFRESH MATERIALIZED VIEW public.records_copy WITH DATA")
            .expect("parse materialized-view refresh");
        let drop_view = parse_statement("DROP MATERIALIZED VIEW IF EXISTS public.records_copy")
            .expect("parse materialized-view drop");

        // Assert
        assert!(matches!(
            refresh.statement,
            QueryStatement::RefreshMaterializedView(statement) if statement.with_data
        ));
        assert!(matches!(
            drop_view.statement,
            QueryStatement::DropMaterializedView(statement) if statement.if_exists
        ));
    }
}
