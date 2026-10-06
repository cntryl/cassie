pub mod aggregate;
mod array_order;
pub mod batch;
mod execution;

pub(crate) fn delete_document_with_referential_actions(
    cassie: &crate::app::Cassie,
    table: &str,
    row_id: &str,
    payload: &serde_json::Value,
    cancellation: crate::runtime::QueryCancellationHandle,
) -> Result<bool, crate::app::CassieError> {
    execution::delete_document_with_referential_actions(
        cassie,
        table,
        row_id,
        payload,
        cancellation,
    )
}
pub mod filter;
pub mod projection;
mod projection_types;
pub(crate) mod retained_memory;
pub mod scan;
pub(crate) mod semantic;
pub mod sort;
mod typed_batch;
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
type RollupPublicationTestBarriers = (
    String,
    u64,
    std::sync::Arc<std::sync::Barrier>,
    std::sync::Arc<std::sync::Barrier>,
);
static ROLLUP_PUBLICATION_BEFORE_REPLACE_BARRIERS: std::sync::OnceLock<
    std::sync::Mutex<Option<RollupPublicationTestBarriers>>,
> = std::sync::OnceLock::new();
static ROLLUP_PUBLICATION_BEFORE_READY_BARRIERS: std::sync::OnceLock<
    std::sync::Mutex<Option<RollupPublicationTestBarriers>>,
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
pub fn set_rollup_publication_before_replace_barriers(
    name: Option<String>,
    source_generation: u64,
    ready: Option<std::sync::Arc<std::sync::Barrier>>,
    resume: Option<std::sync::Arc<std::sync::Barrier>>,
) {
    *ROLLUP_PUBLICATION_BEFORE_REPLACE_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("rollup publication before-replace barrier mutex") = name
        .zip(ready.zip(resume))
        .map(|(name, (ready, resume))| (name, source_generation, ready, resume));
}

#[doc(hidden)]
pub fn set_rollup_publication_before_ready_barriers(
    name: Option<String>,
    source_generation: u64,
    ready: Option<std::sync::Arc<std::sync::Barrier>>,
    resume: Option<std::sync::Arc<std::sync::Barrier>>,
) {
    *ROLLUP_PUBLICATION_BEFORE_READY_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("rollup publication before-ready barrier mutex") = name
        .zip(ready.zip(resume))
        .map(|(name, (ready, resume))| (name, source_generation, ready, resume));
}

fn pause_at_rollup_publication_barrier(
    barriers: &'static std::sync::OnceLock<std::sync::Mutex<Option<RollupPublicationTestBarriers>>>,
    name: &str,
    source_generation: u64,
) {
    let configured = barriers
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("rollup publication test barrier mutex");
    let mut configured = configured;
    let matched = configured
        .as_ref()
        .is_some_and(|(expected_name, expected_generation, _, _)| {
            expected_name == name && *expected_generation == source_generation
        });
    if !matched {
        return;
    }
    if let Some((_, _, ready, resume)) = configured.take() {
        drop(configured);
        ready.wait();
        resume.wait();
    }
}

pub(crate) fn pause_before_rollup_output_replace(name: &str, source_generation: u64) {
    pause_at_rollup_publication_barrier(
        &ROLLUP_PUBLICATION_BEFORE_REPLACE_BARRIERS,
        name,
        source_generation,
    );
}

pub(crate) fn pause_before_rollup_ready_metadata(name: &str, source_generation: u64) {
    pause_at_rollup_publication_barrier(
        &ROLLUP_PUBLICATION_BEFORE_READY_BARRIERS,
        name,
        source_generation,
    );
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

thread_local! {
    static MATERIALIZED_PROJECTION_MAINTENANCE_FAILPOINT: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
    static PROJECTION_ACTIVATION_FAILPOINT: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[doc(hidden)]
pub fn set_materialized_projection_maintenance_failure_point(enabled: bool) {
    MATERIALIZED_PROJECTION_MAINTENANCE_FAILPOINT.set(enabled);
}

pub(crate) fn check_materialized_projection_maintenance_failure_point(
) -> Result<(), crate::app::CassieError> {
    if MATERIALIZED_PROJECTION_MAINTENANCE_FAILPOINT.replace(false) {
        return Err(crate::app::CassieError::Execution(
            "injected test failure during materialized projection maintenance".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_projection_activation_failure_point(enabled: bool) {
    PROJECTION_ACTIVATION_FAILPOINT.set(enabled);
}

pub(crate) fn check_projection_activation_failure_point() -> Result<(), crate::app::CassieError> {
    if PROJECTION_ACTIVATION_FAILPOINT.replace(false) {
        return Err(crate::app::CassieError::Execution(
            "injected projection failure before activation publication".to_string(),
        ));
    }
    Ok(())
}
pub(crate) use execution::resolve_transaction_conflict_intents;
pub(crate) use execution::show_result_columns;
pub(crate) use execution::{
    plan_needs_user_functions, plan_uses_function, plan_uses_function_including_views,
    run_with_session_controls,
};
pub use execution::{
    run, run_with_controls, run_with_execution_breakdown, ColumnMeta, ExecutionBreakdownMicros,
    ExecutionBreakdownOutput, QueryError, QueryResult,
};
pub(crate) use execution::{vector_prefilter_fallback_reason, vector_prefilter_supported};
