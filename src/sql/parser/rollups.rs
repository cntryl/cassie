use super::expr::{parse_expression, split_csv};
use super::schema::{parse_if_exists, parse_if_not_exists};
use super::{ParsedStatement, SqlError, find_top_level_keyword, Expr, parse_projection_items, QueryStatement, CreateRollupStatement, RefreshRollupStatement, DropRollupStatement};

pub(super) fn parse_create_rollup_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["create rollup".len()..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest)?;

    let on_pos = find_top_level_keyword(rest, 0, "on")
        .ok_or_else(|| SqlError("CREATE ROLLUP requires ON source".to_string()))?;
    let name = rest[..on_pos].trim();
    if name.is_empty() {
        return Err(SqlError("CREATE ROLLUP requires a name".to_string()));
    }

    let rest = rest[on_pos + 2..].trim();
    let using_pos = find_top_level_keyword(rest, 0, "using")
        .ok_or_else(|| SqlError("CREATE ROLLUP requires USING time_bucket(...)".to_string()))?;
    let source = rest[..using_pos].trim();
    if source.is_empty() {
        return Err(SqlError(
            "CREATE ROLLUP requires a source collection".to_string(),
        ));
    }

    let rest = rest[using_pos + 5..].trim();
    let group_pos = find_top_level_keyword(rest, 0, "group by")
        .ok_or_else(|| SqlError("CREATE ROLLUP requires GROUP BY".to_string()))?;
    let bucket_raw = rest[..group_pos].trim();
    let Expr::Function(bucket) = parse_expression(bucket_raw)? else {
        return Err(SqlError(
            "CREATE ROLLUP USING requires a function call".to_string(),
        ));
    };

    let rest = rest[group_pos + "group by".len()..].trim();
    let aggregates_pos = find_top_level_keyword(rest, 0, "aggregates")
        .ok_or_else(|| SqlError("CREATE ROLLUP requires AGGREGATES".to_string()))?;
    let group_raw = rest[..aggregates_pos].trim();
    let group_by = if group_raw.is_empty() {
        Vec::new()
    } else {
        split_csv(group_raw)
            .into_iter()
            .map(parse_expression)
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut aggregate_raw = rest[aggregates_pos + "aggregates".len()..].trim();
    let mut filter = None;
    if let Some(where_pos) = find_top_level_keyword(aggregate_raw, 0, "where") {
        let where_raw = aggregate_raw[where_pos + "where".len()..].trim();
        filter = Some(parse_expression(where_raw)?);
        aggregate_raw = aggregate_raw[..where_pos].trim();
    }
    if aggregate_raw.is_empty() {
        return Err(SqlError(
            "CREATE ROLLUP requires aggregate expressions".to_string(),
        ));
    }
    let aggregates = parse_projection_items(aggregate_raw)?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateRollup(CreateRollupStatement {
            name: name.to_string(),
            source: source.to_string(),
            bucket,
            group_by,
            aggregates,
            filter,
            if_not_exists,
        }),
    })
}

pub(super) fn parse_refresh_rollup_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let name = trimmed["refresh rollup".len()..].trim();
    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(SqlError(
            "REFRESH ROLLUP requires one rollup name".to_string(),
        ));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::RefreshRollup(RefreshRollupStatement {
            name: name.to_string(),
        }),
    })
}

pub(super) fn parse_drop_rollup_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["drop rollup".len()..].trim();
    let (if_exists, rest) = parse_if_exists(rest)?;
    let name = rest.trim();
    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(SqlError("DROP ROLLUP requires one rollup name".to_string()));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropRollup(DropRollupStatement {
            name: name.to_string(),
            if_exists,
        }),
    })
}
