use std::sync::atomic::{AtomicBool, Ordering};

pub mod aggregate;
pub mod batch;
mod execution;
pub mod filter;
pub mod projection;
pub mod scan;
pub(crate) mod semantic;
pub mod sort;
mod worker;

pub use aggregate::columns_from_projection;

type MaterializedProjectionReplaceBarriers = (
    std::sync::Arc<std::sync::Barrier>,
    std::sync::Arc<std::sync::Barrier>,
);

static MATERIALIZED_PROJECTION_REPLACE_BARRIERS: std::sync::OnceLock<
    std::sync::Mutex<Option<MaterializedProjectionReplaceBarriers>>,
> = std::sync::OnceLock::new();
static MATERIALIZED_PROJECTION_REPLACE_START_BARRIERS: std::sync::OnceLock<
    std::sync::Mutex<Option<MaterializedProjectionReplaceBarriers>>,
> = std::sync::OnceLock::new();

#[doc(hidden)]
pub fn set_materialized_projection_replace_start_barriers(
    ready: Option<std::sync::Arc<std::sync::Barrier>>,
    resume: Option<std::sync::Arc<std::sync::Barrier>>,
) {
    *MATERIALIZED_PROJECTION_REPLACE_START_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("materialized projection replace start barrier mutex") = ready.zip(resume);
}

#[doc(hidden)]
pub fn set_materialized_projection_replace_barriers(
    dropped: Option<std::sync::Arc<std::sync::Barrier>>,
    resume: Option<std::sync::Arc<std::sync::Barrier>>,
) {
    *MATERIALIZED_PROJECTION_REPLACE_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("materialized projection replace barrier mutex") = dropped.zip(resume);
}

pub(crate) fn pause_after_materialized_projection_drop() {
    let barriers = MATERIALIZED_PROJECTION_REPLACE_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("materialized projection replace barrier mutex")
        .take();
    if let Some((dropped, resume)) = barriers {
        dropped.wait();
        resume.wait();
    }
}

pub(crate) fn pause_before_materialized_projection_replace() {
    let barriers = MATERIALIZED_PROJECTION_REPLACE_START_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("materialized projection replace start barrier mutex")
        .take();
    if let Some((ready, resume)) = barriers {
        ready.wait();
        resume.wait();
    }
}

#[doc(hidden)]
pub fn set_vector_ann_rerank_barriers(
    selected: Option<std::sync::Arc<std::sync::Barrier>>,
    resume: Option<std::sync::Arc<std::sync::Barrier>>,
) {
    let barriers = selected.zip(resume);
    execution::install_ann_rerank_barriers(barriers);
}
pub(crate) use execution::rollup_rewrite_name_for_plan;
pub(crate) use execution::{
    mark_source_projections_stale_external, refresh_rollups_for_source_external,
    sync_derived_maintenance_debt_external,
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
pub(crate) use execution::resolve_transaction_conflict_intents;
pub(crate) use execution::{plan_needs_user_functions, run_with_session_controls};
pub use execution::{
    run, run_with_controls, run_with_execution_breakdown, ColumnMeta, ExecutionBreakdownMicros,
    ExecutionBreakdownOutput, QueryError, QueryResult,
};
pub(crate) use execution::{vector_prefilter_fallback_reason, vector_prefilter_supported};
