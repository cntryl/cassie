use super::schema::{parse_if_exists, parse_if_not_exists};
use super::*;

pub(super) fn parse_create_retention_policy_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["create retention policy".len()..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest)?;

    let on_pos = find_top_level_keyword(rest, 0, "on")
        .ok_or_else(|| SqlError("CREATE RETENTION POLICY requires ON collection".to_string()))?;
    let name = rest[..on_pos].trim();
    if name.is_empty() {
        return Err(SqlError(
            "CREATE RETENTION POLICY requires a name".to_string(),
        ));
    }

    let rest = rest[on_pos + 2..].trim();
    let using_pos = find_top_level_keyword(rest, 0, "using")
        .ok_or_else(|| SqlError("CREATE RETENTION POLICY requires USING field".to_string()))?;
    let collection = rest[..using_pos].trim();
    if collection.is_empty() {
        return Err(SqlError(
            "CREATE RETENTION POLICY requires a collection".to_string(),
        ));
    }

    let rest = rest[using_pos + 5..].trim();
    let retain_pos = find_top_level_keyword(rest, 0, "retain for")
        .ok_or_else(|| SqlError("CREATE RETENTION POLICY requires RETAIN FOR".to_string()))?;
    let timestamp_field = rest[..retain_pos].trim();
    let retention_duration = unquote_required(rest[retain_pos + "retain for".len()..].trim())?;
    if timestamp_field.is_empty() {
        return Err(SqlError(
            "CREATE RETENTION POLICY requires a timestamp field".to_string(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateRetentionPolicy(CreateRetentionPolicyStatement {
            name: name.to_string(),
            collection: collection.to_string(),
            timestamp_field: timestamp_field.to_string(),
            retention_duration,
            if_not_exists,
        }),
    })
}

pub(super) fn parse_alter_retention_policy_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["alter retention policy".len()..].trim();
    let retain_pos = find_top_level_keyword(rest, 0, "retain for")
        .ok_or_else(|| SqlError("ALTER RETENTION POLICY requires RETAIN FOR".to_string()))?;
    let name = rest[..retain_pos].trim();
    if name.is_empty() {
        return Err(SqlError(
            "ALTER RETENTION POLICY requires a name".to_string(),
        ));
    }
    let retention_duration = unquote_required(rest[retain_pos + "retain for".len()..].trim())?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::AlterRetentionPolicy(AlterRetentionPolicyStatement {
            name: name.to_string(),
            retention_duration,
        }),
    })
}

pub(super) fn parse_drop_retention_policy_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["drop retention policy".len()..].trim();
    let (if_exists, rest) = parse_if_exists(rest)?;
    let name = rest.trim();
    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(SqlError(
            "DROP RETENTION POLICY requires one policy name".to_string(),
        ));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropRetentionPolicy(DropRetentionPolicyStatement {
            name: name.to_string(),
            if_exists,
        }),
    })
}

pub(super) fn parse_enforce_retention_policy_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["enforce retention policy".len()..].trim();
    let at_pos = find_top_level_keyword(rest, 0, "at")
        .ok_or_else(|| SqlError("ENFORCE RETENTION POLICY requires AT timestamp".to_string()))?;
    let name = rest[..at_pos].trim();
    if name.is_empty() {
        return Err(SqlError(
            "ENFORCE RETENTION POLICY requires a name".to_string(),
        ));
    }
    let at = unquote_required(rest[at_pos + 2..].trim())?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::EnforceRetentionPolicy(EnforceRetentionPolicyStatement {
            name: name.to_string(),
            at,
        }),
    })
}

fn unquote_required(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        return Ok(raw[1..raw.len() - 1].to_string());
    }
    Err(SqlError(
        "retention values must be single-quoted".to_string(),
    ))
}
