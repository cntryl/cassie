use super::command_output::empty as empty_result;
use time::OffsetDateTime;

#[path = "retention/scheduled.rs"]
mod scheduled;

use super::{
    BatchRow, Cassie, CassieSession, FunctionMeta, HashMap, QueryError, QueryExecutionControls,
    QueryResult, Value, batch, check_timeout, scan,
};

pub(super) fn create_retention_policy(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateRetentionPolicyStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists
        && cassie
            .catalog
            .get_retention_policy(&statement.name)
            .is_some()
    {
        return empty_result("CREATE RETENTION POLICY", controls);
    }

    let metadata = crate::catalog::RetentionPolicyMeta::new(
        statement.name.clone(),
        statement.collection.clone(),
        statement.timestamp_field.clone(),
        statement.retention_duration.clone(),
    );
    let output = empty_result("CREATE RETENTION POLICY", controls)?;
    cassie
        .midge
        .put_retention_policy(&metadata)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_retention_policy(metadata);
    Ok(output)
}

pub(super) fn alter_retention_policy(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::AlterRetentionPolicyStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if !matches!(
        &statement.action,
        crate::sql::ast::AlterRetentionPolicyAction::SetDuration(_)
    ) {
        require_scheduled_policy_admin(session)?;
    }
    let mut metadata = cassie
        .catalog
        .get_retention_policy(&statement.name)
        .ok_or_else(|| {
            QueryError::General(format!(
                "retention policy '{}' does not exist",
                statement.name
            ))
        })?;
    match &statement.action {
        crate::sql::ast::AlterRetentionPolicyAction::SetDuration(duration) => {
            metadata.retention_duration.clone_from(duration);
            metadata.state = crate::catalog::RetentionPolicyState::Ready;
            metadata.last_error = None;
        }
        crate::sql::ast::AlterRetentionPolicyAction::ScheduleDaily { start_at } => {
            if metadata.pending_cutoff_us.is_some() {
                return Err(QueryError::General(
                    "retention schedule cannot be changed while a cutoff is pending".to_owned(),
                ));
            }
            if cassie
                .midge
                .collection_uses_column_store(&metadata.collection)
                .map_err(QueryError::from)?
            {
                return Err(QueryError::General(
                    "scheduled retention requires row storage".to_owned(),
                ));
            }
            let start = parse_timestamp(start_at)?;
            metadata.schedule_state = crate::catalog::RetentionScheduleState::Active;
            metadata.next_due_at_us = Some(timestamp_micros(start)?);
            metadata.state = crate::catalog::RetentionPolicyState::Ready;
            metadata.last_error = None;
        }
        crate::sql::ast::AlterRetentionPolicyAction::Suspend => {
            if metadata.schedule_state == crate::catalog::RetentionScheduleState::Unscheduled {
                return Err(QueryError::General(
                    "retention policy has no schedule to suspend".to_owned(),
                ));
            }
            metadata.schedule_state = crate::catalog::RetentionScheduleState::Suspended;
        }
        crate::sql::ast::AlterRetentionPolicyAction::Resume => {
            if metadata.schedule_state == crate::catalog::RetentionScheduleState::Unscheduled {
                return Err(QueryError::General(
                    "retention policy has no schedule to resume".to_owned(),
                ));
            }
            metadata.schedule_state = crate::catalog::RetentionScheduleState::Active;
        }
    }
    let output = empty_result("ALTER RETENTION POLICY", controls)?;
    cassie
        .midge
        .put_retention_policy(&metadata)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_retention_policy(metadata);
    Ok(output)
}

pub(super) fn drop_retention_policy(
    cassie: &Cassie,
    name: &str,
    if_exists: bool,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if cassie.catalog.get_retention_policy(name).is_none() {
        if if_exists {
            return empty_result("DROP RETENTION POLICY", controls);
        }
        return Err(QueryError::General(format!(
            "retention policy '{name}' does not exist"
        )));
    }
    let output = empty_result("DROP RETENTION POLICY", controls)?;
    cassie
        .midge
        .delete_retention_policy(name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_retention_policy(name);
    Ok(output)
}

pub(super) fn enforce_retention_policy(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::EnforceRetentionPolicyStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    if statement.mode == crate::sql::ast::RetentionEnforcementMode::DueAt {
        require_scheduled_policy_admin(session)?;
    }
    let mut metadata = cassie
        .catalog
        .get_retention_policy(&statement.name)
        .ok_or_else(|| {
            QueryError::General(format!(
                "retention policy '{}' does not exist",
                statement.name
            ))
        })?;

    if statement.mode == crate::sql::ast::RetentionEnforcementMode::DueAt {
        return scheduled::enforce_due(cassie, metadata, &statement.at, controls);
    }
    if metadata.schedule_state != crate::catalog::RetentionScheduleState::Unscheduled
        && metadata.pending_cutoff_us.is_some()
    {
        require_scheduled_policy_admin(session)?;
        return scheduled::replay_pending(cassie, metadata, &statement.at, controls);
    }

    let mut output = QueryResult::try_builder(controls)?;
    super::command_output::set_counted(&mut output, "ENFORCE RETENTION", u64::MAX)?;
    let result = enforce(cassie, &metadata, &statement.at, user_functions, controls);
    match result {
        Ok((deleted, skipped, enforced_ms)) => {
            metadata.state = crate::catalog::RetentionPolicyState::Ready;
            metadata.last_enforced_ms = Some(enforced_ms);
            metadata.last_deleted_rows = deleted;
            metadata.last_skipped_rows = skipped;
            metadata.last_error = None;
            cassie
                .runtime
                .record_retention_enforcement(metadata.name.clone(), deleted, skipped);
            cassie
                .midge
                .put_retention_policy(&metadata)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_retention_policy(metadata);
            super::command_output::set_counted(&mut output, "ENFORCE RETENTION", deleted)?;
            Ok(output.finish()?)
        }
        Err(error) => {
            let message = error.to_string();
            metadata.state = crate::catalog::RetentionPolicyState::Error;
            metadata.last_error = Some(message.clone());
            cassie
                .runtime
                .record_retention_error(metadata.name.clone(), message);
            let _ = cassie.midge.put_retention_policy(&metadata);
            cassie.catalog.register_retention_policy(metadata);
            Err(error)
        }
    }
}

fn enforce(
    cassie: &Cassie,
    metadata: &crate::catalog::RetentionPolicyMeta,
    at: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(u64, u64, u64), QueryError> {
    let enforce_at = parse_timestamp(at)?;
    let duration = parse_duration(&metadata.retention_duration)?;
    let cutoff = enforce_at - duration;
    let collections = cassie.referential_write_collections(&metadata.collection);
    cassie.midge.with_collection_write_gates(&collections, || {
        enforce_with_held_referential_gates(
            cassie,
            metadata,
            cutoff,
            enforce_at,
            user_functions,
            controls,
        )
    })
}

fn enforce_with_held_referential_gates(
    cassie: &Cassie,
    metadata: &crate::catalog::RetentionPolicyMeta,
    cutoff: OffsetDateTime,
    enforce_at: OffsetDateTime,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(u64, u64, u64), QueryError> {
    let rows = batch::flatten_batches(scan::scan(cassie, None, &metadata.collection, controls)?);
    let mut deleted = 0u64;
    let mut skipped = 0u64;

    for row in rows {
        check_timeout(controls)?;
        let row_id = row_id(&row)?;
        let Some(value) = row.get(&metadata.timestamp_field) else {
            skipped += 1;
            continue;
        };
        let raw = match value {
            Value::Typed(typed) => crate::types::extended::temporal_legacy_text(typed),
            other => other.as_str().map(str::to_owned),
        };
        let Some(timestamp) = raw.and_then(|raw| parse_timestamp(&raw).ok()) else {
            skipped += 1;
            continue;
        };
        if timestamp >= cutoff {
            continue;
        }
        let Some(current) = cassie
            .get_document_for_session(None, &metadata.collection, &row_id)
            .map_err(QueryError::from)?
        else {
            continue;
        };
        match super::dml_referential_actions::delete_existing_row(
            cassie,
            None,
            &metadata.collection,
            &row_id,
            &current.payload,
            controls,
        )? {
            super::dml_referential_actions::DeleteOutcome::Deleted(true) => deleted += 1,
            super::dml_referential_actions::DeleteOutcome::Restricted(_) => skipped += 1,
            super::dml_referential_actions::DeleteOutcome::Deleted(false) => {}
        }
    }

    super::rollups::refresh_rollups_for_source(
        cassie,
        &metadata.collection,
        &crate::midge::adapter::documents::RollupSourceChangeReport::default(),
        user_functions,
        controls,
    )?;
    Ok((deleted, skipped, timestamp_millis(enforce_at)))
}

pub(super) fn parse_duration(raw: &str) -> Result<time::Duration, QueryError> {
    let mut parts = raw.split_whitespace();
    let amount = parts
        .next()
        .ok_or_else(|| QueryError::General("retention duration cannot be empty".into()))?
        .parse::<i64>()
        .map_err(|_| QueryError::General("retention duration requires a number".into()))?;
    let unit = parts
        .next()
        .ok_or_else(|| QueryError::General("retention duration requires a unit".into()))?;
    if amount <= 0 || parts.next().is_some() {
        return Err(QueryError::General(
            "retention duration must be '<positive number> <unit>'".into(),
        ));
    }
    match unit.to_ascii_lowercase().as_str() {
        "minute" | "minutes" => Ok(time::Duration::minutes(amount)),
        "hour" | "hours" => Ok(time::Duration::hours(amount)),
        "day" | "days" => Ok(time::Duration::days(amount)),
        _ => Err(QueryError::General(
            "retention duration supports minutes, hours, or days".into(),
        )),
    }
}

pub(super) fn parse_timestamp(raw: &str) -> Result<OffsetDateTime, QueryError> {
    crate::types::temporal::parse_timestamp_utc(raw)
        .map_err(|_| QueryError::General("retention timestamp must be RFC3339".into()))
}

pub(super) fn row_id(row: &BatchRow) -> Result<String, QueryError> {
    match row.get(crate::types::row_identity::ROW_IDENTITY_COLUMN) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(QueryError::General(
            "scanned row is missing internal row id".to_string(),
        )),
    }
}

pub(super) fn timestamp_millis(timestamp: OffsetDateTime) -> u64 {
    u64::try_from(timestamp.unix_timestamp_nanos()).map_or_default(|nanos| nanos / 1_000_000)
}

pub(super) fn timestamp_micros(timestamp: OffsetDateTime) -> Result<i64, QueryError> {
    i64::try_from(timestamp.unix_timestamp_nanos() / 1_000)
        .map_err(|_| QueryError::General("retention timestamp exceeds UTC microseconds".into()))
}

pub(super) fn timestamp_from_micros(value: i64) -> Result<OffsetDateTime, QueryError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(value) * 1_000)
        .map_err(|error| QueryError::General(error.to_string()))
}

fn require_scheduled_policy_admin(session: Option<&CassieSession>) -> Result<(), QueryError> {
    if session.is_some_and(CassieSession::is_authenticated_read_only) {
        return Err(QueryError::from(
            crate::app::CassieError::InsufficientPrivilege,
        ));
    }
    Ok(())
}
