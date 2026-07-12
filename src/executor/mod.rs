use std::sync::atomic::{AtomicBool, Ordering};

pub mod aggregate;
pub mod batch;
mod execution;
pub mod filter;
pub mod projection;
pub mod scan;
pub mod sort;

pub use aggregate::columns_from_projection;
pub(crate) use execution::rollup_rewrite_name_for_plan;
pub(crate) use execution::{
    mark_source_projections_stale_external, refresh_rollups_for_source_external,
};

static MATERIALIZED_PROJECTION_MAINTENANCE_FAILPOINT: AtomicBool = AtomicBool::new(false);

#[doc(hidden)]
pub fn set_materialized_projection_maintenance_failure_point(enabled: bool) {
    MATERIALIZED_PROJECTION_MAINTENANCE_FAILPOINT.store(enabled, Ordering::SeqCst);
}

pub(crate) fn check_materialized_projection_maintenance_failure_point(
) -> Result<(), crate::app::CassieError> {
    if MATERIALIZED_PROJECTION_MAINTENANCE_FAILPOINT.swap(false, Ordering::SeqCst) {
        return Err(crate::app::CassieError::Execution(
            "injected test failure during materialized projection maintenance".to_string(),
        ));
    }
    Ok(())
}
pub(crate) use execution::{plan_needs_user_functions, run_with_session_controls};
pub use execution::{
    run, run_with_controls, run_with_execution_breakdown, ColumnMeta, ExecutionBreakdownMicros,
    ExecutionBreakdownOutput, QueryError, QueryResult,
};
pub(crate) use execution::{vector_prefilter_fallback_reason, vector_prefilter_supported};
