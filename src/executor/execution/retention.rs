use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{Cassie, QueryResult, QueryError, HashMap, FunctionMeta, QueryExecutionControls, check_timeout, batch, scan, BatchRow, Value};

pub(super) fn create_retention_policy(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateRetentionPolicyStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists
        && cassie
            .catalog
            .get_retention_policy(&statement.name)
            .is_some()
    {
        return Ok(empty_result("CREATE RETENTION POLICY"));
    }

    let metadata = crate::catalog::RetentionPolicyMeta::new(
        statement.name.clone(),
        statement.collection.clone(),
        statement.timestamp_field.clone(),
        statement.retention_duration.clone(),
    );
    cassie
        .midge
        .put_retention_policy(metadata.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_retention_policy(metadata);
    Ok(empty_result("CREATE RETENTION POLICY"))
}

pub(super) fn alter_retention_policy(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterRetentionPolicyStatement,
) -> Result<QueryResult, QueryError> {
    let mut metadata = cassie
        .catalog
        .get_retention_policy(&statement.name)
        .ok_or_else(|| {
            QueryError::General(format!(
                "retention policy '{}' does not exist",
                statement.name
            ))
        })?;
    metadata.retention_duration.clone_from(&statement.retention_duration);
    metadata.state = crate::catalog::RetentionPolicyState::Ready;
    metadata.last_error = None;
    cassie
        .midge
        .put_retention_policy(metadata.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_retention_policy(metadata);
    Ok(empty_result("ALTER RETENTION POLICY"))
}

pub(super) fn drop_retention_policy(
    cassie: &Cassie,
    name: &str,
    if_exists: bool,
) -> Result<QueryResult, QueryError> {
    if cassie.catalog.get_retention_policy(name).is_none() {
        if if_exists {
            return Ok(empty_result("DROP RETENTION POLICY"));
        }
        return Err(QueryError::General(format!(
            "retention policy '{name}' does not exist"
        )));
    }
    cassie
        .midge
        .delete_retention_policy(name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_retention_policy(name);
    Ok(empty_result("DROP RETENTION POLICY"))
}

pub(super) fn enforce_retention_policy(
    cassie: &Cassie,
    statement: &crate::sql::ast::EnforceRetentionPolicyStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let mut metadata = cassie
        .catalog
        .get_retention_policy(&statement.name)
        .ok_or_else(|| {
            QueryError::General(format!(
                "retention policy '{}' does not exist",
                statement.name
            ))
        })?;

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
                .put_retention_policy(metadata.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_retention_policy(metadata);
            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: format!("ENFORCE RETENTION {deleted}"),
            })
        }
        Err(error) => {
            let message = error.to_string();
            metadata.state = crate::catalog::RetentionPolicyState::Error;
            metadata.last_error = Some(message.clone());
            cassie
                .runtime
                .record_retention_error(metadata.name.clone(), message);
            let _ = cassie.midge.put_retention_policy(metadata.clone());
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
    let rows = batch::flatten_batches(scan::scan(cassie, None, &metadata.collection)?);
    let mut deleted = 0u64;
    let mut skipped = 0u64;

    for row in rows {
        check_timeout(controls)?;
        let row_id = row_id(&row)?;
        let Some(value) = row.get(&metadata.timestamp_field) else {
            skipped += 1;
            continue;
        };
        let Some(timestamp) = value.as_str().and_then(|raw| parse_timestamp(raw).ok()) else {
            skipped += 1;
            continue;
        };
        if timestamp >= cutoff {
            continue;
        }
        if cassie
            .delete_document_for_session(None, &metadata.collection, &row_id)
            .map_err(|error| QueryError::General(error.to_string()))?
        {
            deleted += 1;
        }
    }

    if deleted > 0 {
        super::materialized_projection::mark_source_projections_stale(
            cassie,
            &metadata.collection,
        )?;
    }
    super::rollups::refresh_rollups_for_source(
        cassie,
        &metadata.collection,
        user_functions,
        controls,
    )?;
    Ok((
        deleted,
        skipped,
        enforce_at.unix_timestamp_nanos() as u64 / 1_000_000,
    ))
}

fn parse_duration(raw: &str) -> Result<time::Duration, QueryError> {
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

fn parse_timestamp(raw: &str) -> Result<OffsetDateTime, QueryError> {
    OffsetDateTime::parse(raw, &Rfc3339)
        .map_err(|_| QueryError::General("retention timestamp must be RFC3339".into()))
}

fn row_id(row: &BatchRow) -> Result<String, QueryError> {
    match row.get("id") {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(QueryError::General(
            "scanned row is missing internal row id".to_string(),
        )),
    }
}

fn empty_result(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
