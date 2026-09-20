use std::collections::HashMap;

use super::materialized_projection::{
    build_specific_version, empty_command, now_ms, persist_projection_metadata,
};
use super::{catalog, Cassie, FunctionMeta, QueryError, QueryExecutionControls, QueryResult};

pub(super) fn repair_materialized_projection_version(
    cassie: &Cassie,
    name: &str,
    version_id: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut metadata = cassie
        .catalog
        .get_materialized_projection(name)
        .ok_or_else(|| {
            QueryError::General(format!("materialized projection '{name}' does not exist"))
        })?;
    let was_active = metadata.active_version.as_deref() == Some(version_id);
    let Some(version) = metadata
        .versions
        .iter_mut()
        .find(|version| version.version_id == version_id)
    else {
        return Err(QueryError::General(format!(
            "projection version '{version_id}' does not exist"
        )));
    };
    version.state = catalog::ProjectionVersionState::Building;
    version.last_error = None;
    version.verification = catalog::ProjectionRebuildVerificationMeta {
        state: catalog::ProjectionVerificationState::Pending,
        ..version.verification.clone()
    };
    persist_projection_metadata(cassie, metadata.clone())?;

    match build_specific_version(cassie, &mut metadata, version_id, user_functions, controls) {
        Ok(()) => {
            if was_active {
                if let Some(version) = metadata
                    .versions
                    .iter_mut()
                    .find(|version| version.version_id == version_id)
                {
                    version.state = catalog::ProjectionVersionState::Active;
                }
            }
            persist_projection_metadata(cassie, metadata)?;
            Ok(empty_command("ALTER MATERIALIZED PROJECTION"))
        }
        Err(error) => {
            if let Some(version) = metadata
                .versions
                .iter_mut()
                .find(|version| version.version_id == version_id)
            {
                version.state = catalog::ProjectionVersionState::Failed;
                version.last_error = Some(error.to_string());
                version.verification.state = catalog::ProjectionVerificationState::Failed;
                version.verification.failure_reason = Some(error.to_string());
                version.verification.completed_ms = Some(now_ms());
            }
            metadata.last_error = Some(error.to_string());
            let _ = persist_projection_metadata(cassie, metadata);
            Err(error)
        }
    }
}

pub(super) fn build_projection_version(
    cassie: &Cassie,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut metadata = cassie
        .catalog
        .get_materialized_projection(name)
        .ok_or_else(|| {
            QueryError::General(format!("materialized projection '{name}' does not exist"))
        })?;
    let materialized = metadata.materialized.clone().ok_or_else(|| {
        QueryError::General(format!(
            "materialized projection '{name}' is missing definition"
        ))
    })?;
    let version_id = metadata.allocate_version_id();
    let output_collection = catalog::materialized_output_collection(name, &version_id);
    if metadata
        .versions
        .iter()
        .any(|version| version.version_id == version_id)
    {
        return Err(QueryError::General(format!(
            "projection version '{version_id}' already exists for '{name}'"
        )));
    }
    if cassie.catalog.exists(&output_collection)
        || cassie.midge.collection_schema(&output_collection).is_some()
    {
        return Err(QueryError::General(format!(
            "output collection '{output_collection}' for projection version '{version_id}' already exists"
        )));
    }
    metadata.versions.push(catalog::ProjectionVersionMeta {
        version_id: version_id.clone(),
        output_collection,
        definition_fingerprint: materialized.definition_fingerprint,
        source_schema_epoch: cassie.catalog.version(),
        state: catalog::ProjectionVersionState::Building,
        created_ms: now_ms(),
        activated_ms: None,
        retired_ms: None,
        last_error: None,
        verification: catalog::ProjectionRebuildVerificationMeta::default(),
    });
    persist_projection_metadata(cassie, metadata.clone())?;

    match build_specific_version(cassie, &mut metadata, &version_id, user_functions, controls) {
        Ok(()) => {
            persist_projection_metadata(cassie, metadata)?;
            Ok(empty_command("ALTER MATERIALIZED PROJECTION"))
        }
        Err(error) => {
            if let Some(version) = metadata
                .versions
                .iter_mut()
                .find(|version| version.version_id == version_id)
            {
                version.state = catalog::ProjectionVersionState::Failed;
                version.last_error = Some(error.to_string());
            }
            metadata.last_error = Some(error.to_string());
            let _ = persist_projection_metadata(cassie, metadata);
            Err(error)
        }
    }
}

pub(super) fn drop_materialized_projection_version(
    cassie: &Cassie,
    name: &str,
    version_id: &str,
) -> Result<QueryResult, QueryError> {
    let metadata = cassie
        .catalog
        .get_materialized_projection(name)
        .ok_or_else(|| {
            QueryError::General(format!("materialized projection '{name}' does not exist"))
        })?;
    if metadata.active_version.as_deref() == Some(version_id) {
        return Err(QueryError::General(format!(
            "cannot drop active projection version '{version_id}'"
        )));
    }
    let Some(index) = metadata
        .versions
        .iter()
        .position(|version| version.version_id == version_id)
    else {
        return Err(QueryError::General(format!(
            "projection version '{version_id}' does not exist"
        )));
    };
    let projection = metadata.collection.clone();
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&projection), || {
            drop_materialized_projection_version_gated(cassie, metadata, index, &projection)
        })?;
    Ok(empty_command("DROP MATERIALIZED PROJECTION VERSION"))
}

/// Removes one projection version while holding the projection's catalog gate,
/// so the report ids read from the catalog cannot change before deletion.
///
/// Stored reports go first: if their deletion fails, the version, its output,
/// and its metadata are all still intact. The metadata is persisted before the
/// output collection is dropped, because a leftover output collection is
/// recoverable while a version whose output is gone is not.
fn drop_materialized_projection_version_gated(
    cassie: &Cassie,
    mut metadata: catalog::ProjectionMeta,
    index: usize,
    projection: &str,
) -> Result<(), QueryError> {
    let version_id = metadata.versions[index].version_id.clone();
    let repair_reports = cassie
        .catalog
        .projection_repair_report_ids_for_version(projection, &version_id);
    let comparison_reports = cassie
        .catalog
        .projection_comparison_report_ids_for_version(projection, &version_id);
    // Delete the stored reports first: if that fails, the version, its output,
    // and its metadata are still intact.
    cassie
        .midge
        .delete_projection_reports(&repair_reports, &comparison_reports)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .unregister_projection_repair_reports(&repair_reports);
    cassie
        .catalog
        .unregister_projection_comparison_reports(&comparison_reports);
    let version = metadata.versions.remove(index);
    persist_projection_metadata(cassie, metadata)?;
    if let Err(error) = cassie.midge.drop_collection(&version.output_collection) {
        tracing::warn!(
            collection = version.output_collection.as_str(),
            error = error.to_string().as_str(),
            "dropping a projection version left its output collection behind"
        );
    }
    let _ = cassie
        .catalog
        .unregister_collection(&version.output_collection);
    Ok(())
}
