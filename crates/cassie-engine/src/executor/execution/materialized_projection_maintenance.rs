use std::fmt::Display;

use super::{catalog, materialized_projection, Cassie, QueryError};

const MATERIALIZED_PROJECTION_ARTIFACT: &str = "materialized_projection";

pub(super) fn mark_source_projections_stale(
    cassie: &Cassie,
    source: &str,
) -> Result<(), QueryError> {
    let source = cassie
        .catalog
        .get_schema(source)
        .map_or_else(|| source.to_string(), |schema| schema.collection);
    let mut affected = cassie
        .catalog
        .list_projection_metadata()
        .into_iter()
        .filter(|projection| {
            projection
                .materialized
                .as_ref()
                .is_some_and(|materialized| {
                    materialized
                        .source_collections
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(&source))
                })
        })
        .collect::<Vec<_>>();
    if affected.is_empty() {
        return Ok(());
    }

    let generation = cassie
        .midge
        .collection_generation(&source)
        .map_err(QueryError::Cassie)?;
    if let Err(error) = crate::executor::check_materialized_projection_maintenance_failure_point() {
        record_failure_and_sync(cassie, &source, generation, &error);
        return Ok(());
    }

    for projection in &mut affected {
        let Some(materialized) = projection.materialized.as_mut() else {
            continue;
        };
        materialized.state = catalog::MaterializedProjectionState::Stale;
        projection.freshness = catalog::ProjectionFreshness::Stale;
        projection.hashes.rows.state = catalog::ProjectionVerificationState::Stale;
        projection.hashes.ranges.state = catalog::ProjectionVerificationState::Stale;
        projection.hashes.root.state = catalog::ProjectionVerificationState::Stale;
        projection.verification.state = catalog::ProjectionVerificationState::Pending;
        projection.lag = projection.lag.saturating_add(1);
        cassie
            .runtime
            .record_projection_stale_mark(projection.collection.clone());
        if let Err(error) =
            materialized_projection::persist_projection_metadata(cassie, projection.clone())
        {
            record_failure_and_sync(cassie, &source, generation, &error);
            return Ok(());
        }
    }

    let _ = cassie
        .midge
        .clear_materialized_projection_maintenance_debt(&source, generation);
    let _ = sync_debt_catalog(cassie, &source);
    Ok(())
}

fn record_failure_and_sync(cassie: &Cassie, source: &str, generation: u64, error: &impl Display) {
    let error = crate::app::CassieError::Execution(error.to_string());
    let _ = cassie
        .midge
        .record_materialized_projection_maintenance_failure(source, generation, &error);
    let _ = sync_debt_catalog(cassie, source);
}

pub(super) fn sync_debt_catalog(cassie: &Cassie, source: &str) -> Result<(), QueryError> {
    let Some(debt) = cassie
        .midge
        .maintenance_debt_for(source, MATERIALIZED_PROJECTION_ARTIFACT)
        .map_err(QueryError::Cassie)?
    else {
        cassie
            .catalog
            .unregister_maintenance_debt(source, MATERIALIZED_PROJECTION_ARTIFACT);
        return Ok(());
    };
    cassie
        .catalog
        .register_maintenance_debt(catalog::MaintenanceDebtMeta::new(
            debt.collection,
            debt.artifact,
            debt.target_generation,
            debt.retry_count,
            debt.last_error,
        ));
    Ok(())
}
