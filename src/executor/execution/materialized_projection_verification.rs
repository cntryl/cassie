//! Verification report preparation shared by public VERIFY and private repair work.
use super::super::{check_timeout, command_output};
use super::{
    catalog, now_ms, persist_projection_metadata, Cassie, QueryError, QueryExecutionControls,
    QueryResult,
};
use crate::midge::adapter::IntegrityCheckReport;
use crate::types::DataType;

pub(in crate::executor::execution) fn verify_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::VerifyProjectionStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let verification = prepare_verification(cassie, statement)?;
    let target = &verification.target;
    let report = &verification.report;
    let mut result = command_output::columns(
        "VERIFY PROJECTION",
        &[
            ("state", DataType::Text),
            ("target_collection", DataType::Text),
            ("mode", DataType::Text),
            ("mismatch_count", DataType::BigInt),
            ("missing_count", DataType::BigInt),
            ("stale_count", DataType::BigInt),
            ("repairable", DataType::Boolean),
            ("checked_components", DataType::Text),
            ("skipped_components", DataType::Text),
            ("last_error", DataType::Text),
        ],
        controls,
    )?;
    use command_output::Cell;
    command_output::push_row(
        &mut result,
        &[
            Cell::Text(report.state.as_str()),
            Cell::Text(&target.output_collection),
            Cell::Text(statement.mode.as_str()),
            Cell::Int(u64_to_i64(report.mismatch_count)),
            Cell::Int(u64_to_i64(report.missing_count)),
            Cell::Int(u64_to_i64(report.stale_count)),
            Cell::Bool(report.repairable),
            Cell::Joined(&report.checked_components),
            Cell::Joined(&report.skipped_components),
            Cell::Text(report.last_error.as_deref().unwrap_or_default()),
        ],
        controls,
    )?;
    let output = result.finish()?;
    publish_verification(cassie, verification)?;
    Ok(output)
}

pub(in crate::executor::execution) fn verify_projection_state(
    cassie: &Cassie,
    statement: &crate::sql::ast::VerifyProjectionStatement,
    controls: &QueryExecutionControls,
) -> Result<catalog::ProjectionVerificationState, QueryError> {
    let verification = prepare_verification(cassie, statement)?;
    check_timeout(controls)?;
    publish_verification(cassie, verification)
}

struct Verification {
    target: VerificationTarget,
    report: IntegrityCheckReport,
    projection: catalog::ProjectionMeta,
}

fn prepare_verification(
    cassie: &Cassie,
    statement: &crate::sql::ast::VerifyProjectionStatement,
) -> Result<Verification, QueryError> {
    let target = resolve_verification_target(cassie, statement)?;
    let (metadata, hashes, indexes) = verification_mode_components(statement.mode);
    let report = cassie
        .midge
        .verify_projection_integrity(&target.output_collection, hashes, indexes, metadata)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let completed_ms = Some(now_ms());
    let persisted = catalog::ProjectionIntegrityReportMeta {
        state: report.state.clone(),
        target: Some(target.output_collection.clone()),
        version_id: target.version_id.clone(),
        mode: statement.mode.as_str().to_string(),
        checked_components: report.checked_components.clone(),
        skipped_components: report.skipped_components.clone(),
        mismatch_count: report.mismatch_count,
        missing_count: report.missing_count,
        stale_count: report.stale_count,
        repairable: report.repairable,
        elapsed_ms: report.elapsed_ms,
        completed_ms,
        last_error: report.last_error.clone(),
    };
    let mut projection = cassie
        .catalog
        .get_projection_metadata(&target.metadata_key)
        .or_else(|| {
            cassie
                .catalog
                .get_materialized_projection(&target.metadata_key)
        })
        .unwrap_or_else(|| catalog::ProjectionMeta::new(&target.metadata_key, 1));
    projection.integrity = persisted;
    Ok(Verification {
        target,
        report,
        projection,
    })
}

fn publish_verification(
    cassie: &Cassie,
    verification: Verification,
) -> Result<catalog::ProjectionVerificationState, QueryError> {
    let state = verification.report.state;
    persist_projection_metadata(cassie, verification.projection)?;
    cassie.runtime.record_projection_integrity_verification(
        verification.target.metadata_key,
        state != catalog::ProjectionVerificationState::Verified,
    );
    Ok(state)
}

struct VerificationTarget {
    metadata_key: String,
    output_collection: String,
    version_id: Option<String>,
}

fn resolve_verification_target(
    cassie: &Cassie,
    statement: &crate::sql::ast::VerifyProjectionStatement,
) -> Result<VerificationTarget, QueryError> {
    if let Some(projection) = cassie.catalog.get_materialized_projection(&statement.name) {
        let version_id = statement
            .version_id
            .clone()
            .or_else(|| projection.active_version.clone());
        let output_collection = if let Some(version_id) = version_id.as_ref() {
            projection
                .versions
                .iter()
                .find(|version| &version.version_id == version_id)
                .map(|version| version.output_collection.clone())
                .ok_or_else(|| {
                    QueryError::General(format!("projection version '{version_id}' does not exist"))
                })?
        } else {
            projection
                .materialized
                .as_ref()
                .map(|materialized| materialized.output_collection.clone())
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "materialized projection '{}' is missing output collection",
                        statement.name
                    ))
                })?
        };
        return Ok(VerificationTarget {
            metadata_key: projection.collection,
            output_collection,
            version_id,
        });
    }
    if !cassie.catalog.exists(&statement.name) {
        return Err(QueryError::General(format!(
            "projection or collection '{}' does not exist",
            statement.name
        )));
    }
    Ok(VerificationTarget {
        metadata_key: statement.name.clone(),
        output_collection: statement.name.clone(),
        version_id: statement.version_id.clone(),
    })
}

fn verification_mode_components(
    mode: crate::sql::ast::ProjectionVerificationMode,
) -> (bool, bool, bool) {
    match mode {
        crate::sql::ast::ProjectionVerificationMode::MetadataOnly => (true, false, false),
        crate::sql::ast::ProjectionVerificationMode::HashesOnly => (false, true, false),
        crate::sql::ast::ProjectionVerificationMode::IndexesOnly => (false, false, true),
        crate::sql::ast::ProjectionVerificationMode::Full => (true, true, true),
    }
}

fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
