use std::time::{SystemTime, UNIX_EPOCH};

use super::{catalog, Cassie, ColumnMeta, DataType, QueryError, QueryResult, Value};
use crate::sql::ast::{
    ProjectionDiffTarget, ProjectionRepairScope, ProjectionVerificationMode,
    RepairProjectionStatement, VerifyProjectionStatement,
};

#[derive(Debug, Clone)]
struct RepairPlan {
    projection_name: String,
    target_collection: String,
    version_id: Option<String>,
    scope: ProjectionRepairScope,
    action: String,
    executable: bool,
    affected_objects: Vec<String>,
    source_report_state: String,
    source_mismatch_count: u64,
    source_missing_count: u64,
    source_stale_count: u64,
    verification_required: String,
    post_verification_state: String,
    last_error: Option<String>,
}

pub(super) fn plan_repair_projection(
    cassie: &Cassie,
    target: &ProjectionDiffTarget,
    scope: ProjectionRepairScope,
) -> Result<QueryResult, QueryError> {
    let mut plan = build_repair_plan(cassie, target, scope)?;
    plan.post_verification_state = "pending".to_string();
    Ok(repair_result(
        "PLAN REPAIR PROJECTION",
        "planned",
        "",
        &plan,
    ))
}

pub(super) fn repair_projection(
    cassie: &Cassie,
    statement: &RepairProjectionStatement,
) -> Result<QueryResult, QueryError> {
    let mut plan = build_repair_plan(cassie, &statement.target, statement.scope)?;
    if !plan.executable {
        return Err(QueryError::General(format!(
            "repair scope '{}' is not executable by Cassie",
            statement.scope.as_str()
        )));
    }

    execute_repair_action(cassie, &plan)?;
    let verification = super::materialized_projection::verify_projection(
        cassie,
        &VerifyProjectionStatement {
            name: statement.target.name.clone(),
            version_id: statement.target.version_id.clone(),
            mode: ProjectionVerificationMode::Full,
        },
    )?;
    plan.post_verification_state = verification
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|value| match value {
            Value::String(state) => Some(state.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "unknown".to_string());
    if plan.post_verification_state != "verified" {
        return Err(QueryError::General(format!(
            "repair scope '{}' failed post-verification with state '{}'",
            statement.scope.as_str(),
            plan.post_verification_state
        )));
    }

    let report_id = format!(
        "repair-{}-{}-{}",
        plan.projection_name,
        statement.scope.as_str(),
        now_ms()
    );
    let report = catalog::ProjectionRepairReportMeta {
        report_id: report_id.clone(),
        created_ms: now_ms(),
        projection_name: plan.projection_name.clone(),
        target: plan.target_collection.clone(),
        version_id: plan.version_id.clone(),
        scope: plan.scope.as_str().to_string(),
        action: plan.action.clone(),
        state: "completed".to_string(),
        executable: true,
        affected_objects: plan.affected_objects.clone(),
        source_report_state: plan.source_report_state.clone(),
        source_mismatch_count: plan.source_mismatch_count,
        source_missing_count: plan.source_missing_count,
        source_stale_count: plan.source_stale_count,
        verification_required: plan.verification_required.clone(),
        post_verification_state: plan.post_verification_state.clone(),
        last_error: None,
    };
    cassie
        .midge
        .put_projection_repair_report(&report)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_projection_repair_report(report);

    Ok(repair_result(
        "REPAIR PROJECTION",
        "completed",
        &report_id,
        &plan,
    ))
}

fn build_repair_plan(
    cassie: &Cassie,
    target: &ProjectionDiffTarget,
    scope: ProjectionRepairScope,
) -> Result<RepairPlan, QueryError> {
    let metadata = projection_metadata(cassie, &target.name)?;
    let integrity = metadata.integrity.clone();
    if integrity.completed_ms.is_none() && integrity.mode.is_empty() {
        return Err(QueryError::General(format!(
            "repair planning for '{}' requires a prior VERIFY PROJECTION report",
            target.name
        )));
    }
    let target_collection = integrity
        .target
        .clone()
        .or_else(|| active_target_collection(&metadata, target.version_id.as_deref()))
        .unwrap_or_else(|| target.name.clone());
    let version_id = target.version_id.clone().or(integrity.version_id.clone());
    let source_report_state = integrity.state.as_str().to_string();
    let affected_objects = vec![
        format!("mismatch_count={}", integrity.mismatch_count),
        format!("missing_count={}", integrity.missing_count),
        format!("stale_count={}", integrity.stale_count),
    ];
    let action = action_for_scope(scope).to_string();
    let executable = match scope {
        ProjectionRepairScope::Row | ProjectionRepairScope::Range => integrity.repairable,
        ProjectionRepairScope::Index => {
            integrity.repairable
                && integrity.mode == ProjectionVerificationMode::IndexesOnly.as_str()
        }
        ProjectionRepairScope::ProjectionVersion => false,
        ProjectionRepairScope::FullRebuild => {
            metadata.kind == catalog::ProjectionKind::Materialized
                && integrity.repairable
                && integrity.mode == ProjectionVerificationMode::Full.as_str()
                && metadata.active_version.as_deref() == version_id.as_deref()
        }
    };

    if matches!(
        scope,
        ProjectionRepairScope::Row | ProjectionRepairScope::Range
    ) && !integrity.repairable
    {
        return Err(QueryError::General(format!(
            "latest integrity report for '{}' has no repairable row or range findings",
            target.name
        )));
    }

    Ok(RepairPlan {
        projection_name: target.name.clone(),
        target_collection,
        version_id,
        scope,
        action,
        executable,
        affected_objects,
        source_report_state,
        source_mismatch_count: integrity.mismatch_count,
        source_missing_count: integrity.missing_count,
        source_stale_count: integrity.stale_count,
        verification_required: verification_command(target),
        post_verification_state: "not_run".to_string(),
        last_error: None,
    })
}

fn execute_repair_action(cassie: &Cassie, plan: &RepairPlan) -> Result<(), QueryError> {
    match plan.scope {
        ProjectionRepairScope::Row | ProjectionRepairScope::Range => cassie
            .midge
            .rebuild_projection_hashes(&plan.target_collection)
            .map(|_| ())
            .map_err(|error| QueryError::General(error.to_string())),
        ProjectionRepairScope::Index => cassie
            .midge
            .with_collection_write_gates(std::slice::from_ref(&plan.target_collection), || {
                execute_index_repair(cassie, &plan.target_collection)
            }),
        ProjectionRepairScope::ProjectionVersion => Err(QueryError::General(format!(
            "repair scope '{}' is not executable by Cassie",
            plan.scope.as_str()
        ))),
        ProjectionRepairScope::FullRebuild => execute_full_rebuild_repair(cassie, plan),
    }
}

fn execute_full_rebuild_repair(cassie: &Cassie, plan: &RepairPlan) -> Result<(), QueryError> {
    let metadata = cassie
        .catalog
        .get_materialized_projection(&plan.projection_name)
        .ok_or_else(|| {
            QueryError::General(format!(
                "materialized projection '{}' does not exist",
                plan.projection_name
            ))
        })?;
    let mut gated_collections = vec![plan.target_collection.clone()];
    if let Some(materialized) = metadata.materialized.as_ref() {
        gated_collections.extend(materialized.source_collections.iter().cloned());
    }
    let user_functions = cassie
        .catalog
        .list_functions()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function))
        .collect::<std::collections::HashMap<_, _>>();
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    cassie
        .midge
        .with_collection_write_gates(&gated_collections, || {
            super::materialized_projection::refresh_materialized_projection(
                cassie,
                None,
                &plan.projection_name,
                &user_functions,
                &controls,
            )
            .map(|_| ())
        })
}

fn execute_index_repair(cassie: &Cassie, collection: &str) -> Result<(), QueryError> {
    cassie
        .midge
        .rebuild_scalar_indexes_for_collection(collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .midge
        .rebuild_time_series_indexes_for_collection(collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .midge
        .rebuild_column_batches_for_collection(collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let vector_indexes = cassie
        .midge
        .list_vector_indexes()
        .map_err(|error| QueryError::General(error.to_string()))?;
    for index in vector_indexes
        .into_iter()
        .filter(|index| index.collection.eq_ignore_ascii_case(collection))
    {
        cassie
            .midge
            .rebuild_normalized_vectors_for_index(&index)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    cassie
        .midge
        .refresh_hnsw_indexes_for_collection(collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .midge
        .refresh_ivfflat_indexes_for_collection(collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(())
}

fn projection_metadata(cassie: &Cassie, name: &str) -> Result<catalog::ProjectionMeta, QueryError> {
    cassie
        .catalog
        .get_materialized_projection(name)
        .or_else(|| cassie.catalog.get_projection_metadata(name))
        .ok_or_else(|| {
            QueryError::General(format!("projection or collection '{name}' does not exist"))
        })
}

fn active_target_collection(
    metadata: &catalog::ProjectionMeta,
    version_id: Option<&str>,
) -> Option<String> {
    if metadata.kind != catalog::ProjectionKind::Materialized {
        return Some(metadata.collection.clone());
    }
    let version_id = version_id.or(metadata.active_version.as_deref());
    if let Some(version_id) = version_id {
        return metadata
            .versions
            .iter()
            .find(|version| version.version_id == version_id)
            .map(|version| version.output_collection.clone());
    }
    metadata
        .materialized
        .as_ref()
        .map(|materialized| materialized.output_collection.clone())
}

fn action_for_scope(scope: ProjectionRepairScope) -> &'static str {
    match scope {
        ProjectionRepairScope::Row | ProjectionRepairScope::Range => "rebuild_projection_hashes",
        ProjectionRepairScope::Index => "rebuild_index_entries",
        ProjectionRepairScope::ProjectionVersion => "refresh_projection_version",
        ProjectionRepairScope::FullRebuild => "refresh_materialized_projection",
    }
}

fn verification_command(target: &ProjectionDiffTarget) -> String {
    if let Some(version_id) = target.version_id.as_ref() {
        format!(
            "VERIFY PROJECTION {} VERSION {} MODE full",
            target.name, version_id
        )
    } else {
        format!("VERIFY PROJECTION {} MODE full", target.name)
    }
}

fn repair_result(command: &str, state: &str, report_id: &str, plan: &RepairPlan) -> QueryResult {
    QueryResult {
        columns: vec![
            ColumnMeta::text("state"),
            ColumnMeta::text("projection_name"),
            ColumnMeta::text("scope"),
            ColumnMeta::text("target_collection"),
            ColumnMeta::text("action"),
            ColumnMeta::from_data_type("executable", &DataType::Boolean),
            ColumnMeta::text("verification_required"),
            ColumnMeta::text("post_verification_state"),
            ColumnMeta::text("report_id"),
            ColumnMeta::text("affected_objects"),
            ColumnMeta::text("last_error"),
        ],
        rows: vec![vec![
            Value::String(state.to_string()),
            Value::String(plan.projection_name.clone()),
            Value::String(plan.scope.as_str().to_string()),
            Value::String(plan.target_collection.clone()),
            Value::String(plan.action.clone()),
            Value::Bool(plan.executable),
            Value::String(plan.verification_required.clone()),
            Value::String(plan.post_verification_state.clone()),
            Value::String(report_id.to_string()),
            Value::String(plan.affected_objects.join(",")),
            Value::String(plan.last_error.clone().unwrap_or_default()),
        ]],
        command: command.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}
