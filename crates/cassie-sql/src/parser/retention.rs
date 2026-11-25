use super::schema::{parse_if_exists, parse_if_not_exists};
use super::{
    find_top_level_keyword, AlterRetentionPolicyStatement, CreateRetentionPolicyStatement,
    DropRetentionPolicyStatement, EnforceRetentionPolicyStatement, ParsedStatement, QueryStatement,
    SqlError,
};
use crate::ast::{AlterRetentionPolicyAction, RetentionEnforcementMode};

pub(super) fn parse_create_retention_policy_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["create retention policy".len()..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest);

    let on_pos = find_top_level_keyword(rest, 0, "on").ok_or_else(|| {
        SqlError::new("CREATE RETENTION POLICY requires ON collection".to_string())
    })?;
    let name = rest[..on_pos].trim();
    if name.is_empty() {
        return Err(SqlError::new(
            "CREATE RETENTION POLICY requires a name".to_string(),
        ));
    }

    let rest = rest[on_pos + 2..].trim();
    let using_pos = find_top_level_keyword(rest, 0, "using")
        .ok_or_else(|| SqlError::new("CREATE RETENTION POLICY requires USING field".to_string()))?;
    let collection = rest[..using_pos].trim();
    if collection.is_empty() {
        return Err(SqlError::new(
            "CREATE RETENTION POLICY requires a collection".to_string(),
        ));
    }

    let rest = rest[using_pos + 5..].trim();
    let (retain_pos, retain_end) =
        super::clauses::find_top_level_keyword_span(rest, 0, "retain for").ok_or_else(|| {
            SqlError::new("CREATE RETENTION POLICY requires RETAIN FOR".to_string())
        })?;
    let timestamp_field = rest[..retain_pos].trim();
    let retention_duration = unquote_required(rest[retain_end..].trim())?;
    if timestamp_field.is_empty() {
        return Err(SqlError::new(
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
    let (name_end, action) = if let Some((retain_pos, retain_end)) =
        super::clauses::find_top_level_keyword_span(rest, 0, "retain for")
    {
        (
            retain_pos,
            AlterRetentionPolicyAction::SetDuration(unquote_required(rest[retain_end..].trim())?),
        )
    } else if let Some(schedule_pos) = find_top_level_keyword(rest, 0, "schedule daily start at") {
        let start_at =
            unquote_required(rest[schedule_pos + "schedule daily start at".len()..].trim())?;
        (
            schedule_pos,
            AlterRetentionPolicyAction::ScheduleDaily { start_at },
        )
    } else if let Some(suspend_pos) = find_top_level_keyword(rest, 0, "suspend") {
        if !rest[suspend_pos + "suspend".len()..].trim().is_empty() {
            return Err(SqlError::new(
                "ALTER RETENTION POLICY SUSPEND does not accept a value".to_string(),
            ));
        }
        (suspend_pos, AlterRetentionPolicyAction::Suspend)
    } else if let Some(resume_pos) = find_top_level_keyword(rest, 0, "resume") {
        if !rest[resume_pos + "resume".len()..].trim().is_empty() {
            return Err(SqlError::new(
                "ALTER RETENTION POLICY RESUME does not accept a value".to_string(),
            ));
        }
        (resume_pos, AlterRetentionPolicyAction::Resume)
    } else {
        return Err(SqlError::new(
            "ALTER RETENTION POLICY requires RETAIN FOR, SCHEDULE DAILY START AT, SUSPEND, or RESUME"
                .to_string(),
        ));
    };
    let name = rest[..name_end].trim();
    if name.is_empty() {
        return Err(SqlError::new(
            "ALTER RETENTION POLICY requires a name".to_string(),
        ));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::AlterRetentionPolicy(AlterRetentionPolicyStatement {
            name: name.to_string(),
            action,
        }),
    })
}

pub(super) fn parse_drop_retention_policy_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["drop retention policy".len()..].trim();
    let (if_exists, rest) = parse_if_exists(rest);
    let name = rest.trim();
    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(SqlError::new(
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
    let (at_pos, mode, keyword) = if let Some(position) = find_top_level_keyword(rest, 0, "due at")
    {
        (position, RetentionEnforcementMode::DueAt, "due at")
    } else if let Some(position) = find_top_level_keyword(rest, 0, "at") {
        (position, RetentionEnforcementMode::At, "at")
    } else {
        return Err(SqlError::new(
            "ENFORCE RETENTION POLICY requires AT or DUE AT timestamp".to_string(),
        ));
    };
    let name = rest[..at_pos].trim();
    if name.is_empty() {
        return Err(SqlError::new(
            "ENFORCE RETENTION POLICY requires a name".to_string(),
        ));
    }
    let at = unquote_required(rest[at_pos + keyword.len()..].trim())?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::EnforceRetentionPolicy(EnforceRetentionPolicyStatement {
            name: name.to_string(),
            at,
            mode,
        }),
    })
}

fn unquote_required(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        return Ok(raw[1..raw.len() - 1].to_string());
    }
    Err(SqlError::new(
        "retention values must be single-quoted".to_string(),
    ))
}
