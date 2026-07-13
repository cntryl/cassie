use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::executor::QueryResult;
use crate::planner::physical::PhysicalPlan;
use crate::search::analyzer::AnalyzerConfig;
use crate::types::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionMode {
    SimpleQuery,
    DescribeQuery,
    ExtendedQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlanCacheKey {
    pub sql_fingerprint: u64,
    pub schema_epoch: u64,
    pub data_epoch: u64,
    pub index_feedback_epoch: u64,
    pub cost_model_version: u32,
    #[serde(default)]
    pub adaptive_config_hash: u64,
    pub parameter_shape: Vec<ParameterShape>,
    pub mode: ExecutionMode,
    pub database: Option<String>,
    #[serde(default)]
    pub search_path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParameterShape {
    Null,
    Bool,
    Int64,
    Float64,
    String,
    Vector(usize),
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExecutionResultCacheKey {
    pub sql_fingerprint: u64,
    pub params_hash: u64,
    pub schema_epoch: u64,
    pub data_epoch: u64,
    pub database: Option<String>,
    pub search_path: Vec<String>,
    pub mode: ExecutionMode,
}

#[path = "runtime/adaptive_metrics.rs"]
mod adaptive_metrics;
#[path = "runtime/cache_state.rs"]
mod cache_state;
#[path = "runtime/column_batch_metrics.rs"]
mod column_batch_metrics;
#[path = "runtime/controls.rs"]
mod controls;
#[path = "runtime/feedback.rs"]
mod feedback;
#[path = "runtime/fulltext.rs"]
mod fulltext;
#[path = "runtime/helpers.rs"]
mod helpers;
#[path = "runtime/join_metrics.rs"]
mod join_metrics;
#[path = "runtime/operator_feedback_state.rs"]
mod operator_feedback_state;
#[path = "runtime/projection_metrics.rs"]
mod projection_metrics;
#[path = "runtime/query_cache.rs"]
pub(crate) mod query_cache;
#[path = "runtime/read_path_metrics.rs"]
mod read_path_metrics;
#[path = "runtime/retention_metrics.rs"]
mod retention_metrics;
#[path = "runtime/rollup_metrics.rs"]
mod rollup_metrics;
#[path = "runtime/schema_epochs.rs"]
mod schema_epochs;
#[path = "runtime/snapshot_metrics.rs"]
mod snapshot_metrics;
#[path = "runtime/snapshots.rs"]
mod snapshots;
#[path = "runtime/time_series_metrics.rs"]
mod time_series_metrics;
#[path = "runtime/vector_metrics.rs"]
mod vector_metrics;

pub use controls::QueryExecutionControls;
pub(crate) use feedback::{
    normalized_feedback_key, observation_is_outlier, recompute_feedback_confidence,
    OperatorFeedbackEstimate, RuntimeFeedbackLookup, RuntimeFeedbackLookupState,
    OPERATOR_FEEDBACK_CONFIDENCE_FLOOR_BPS, OPERATOR_FEEDBACK_MIN_STABLE_SAMPLES,
};
pub use feedback::{RuntimeFeedbackKey, RuntimeFeedbackObservation, RuntimeFeedbackRecord};
pub use fulltext::{FulltextIndexOptions, FulltextIndexOptionsCacheKey};
pub(crate) use helpers::stable_fingerprint;
use helpers::{
    adjust_signed, apply_feedback_observation, current_time_millis, duration_ms,
    prune_feedback_by_age, status_class, touch, touch_feedback,
};
pub use helpers::{error_class, hash_params, parameter_shape, sql_fingerprint};
pub(crate) use join_metrics::VectorizedJoinInputRows;
pub(crate) use projection_metrics::ProjectionWriteStats;
pub use schema_epochs::RunningQueryGuard;
use schema_epochs::SchemaEpochTracker;
pub use snapshots::*;

#[derive(Debug, Default)]
struct ExecutionResultCacheState {
    entries: HashMap<ExecutionResultCacheKey, QueryResult>,
    order: VecDeque<ExecutionResultCacheKey>,
}

#[derive(Debug, Default)]
struct RuntimeFeedbackState {
    entries: HashMap<RuntimeFeedbackKey, RuntimeFeedbackRecord>,
    order: VecDeque<RuntimeFeedbackKey>,
}

#[derive(Debug, Default)]
struct RuntimeMetricsState {
    runtime: RuntimeSnapshot,
    query: QuerySnapshot,
    rest: RestSnapshot,
    pgwire: PgwireSnapshot,
    search: ExecutionSnapshot,
    vector: ExecutionSnapshot,
    hybrid: ExecutionSnapshot,
    storage: StorageSnapshot,
    plan_cache: PlanCacheSnapshot,
    query_cache: QueryCacheSnapshot,
    cardinality: CardinalitySnapshot,
    feedback: FeedbackSnapshot,
    adaptive_candidates: AdaptiveCandidateSnapshot,
    joins: JoinSnapshot,
    covering_indexes: CoveringIndexSnapshot,
    column_batches: ColumnBatchSnapshot,
    time_series: TimeSeriesSnapshot,
    aggregate_acceleration: AggregateAccelerationSnapshot,
    parallel_scans: ParallelScanSnapshot,
    parallel_scoring: ParallelScoringSnapshot,
    parallel_aggregation: ParallelAggregationSnapshot,
    rollups: RollupSnapshot,
    projections: ProjectionSnapshot,
    retention: RetentionSnapshot,
    read_paths: ReadPathSnapshot,
    graph: GraphSnapshot,
}

#[derive(Debug, Clone)]
struct L1PlanEntry {
    plan: Arc<PhysicalPlan>,
    durable: bool,
    candidate_expires_at_ms: Option<u64>,
}

#[derive(Debug, Default)]
struct PlanCacheState {
    entries: HashMap<PlanCacheKey, L1PlanEntry>,
    order: VecDeque<PlanCacheKey>,
}

#[derive(Debug, Clone)]
pub struct L1PlanHit {
    pub plan: Arc<PhysicalPlan>,
    pub durable: bool,
    pub candidate_expires_at_ms: Option<u64>,
}

#[derive(Debug)]
pub struct RuntimeState {
    limits: CassieRuntimeLimits,
    metrics: Mutex<RuntimeMetricsState>,
    plan_cache: Mutex<PlanCacheState>,
    feedback: Mutex<RuntimeFeedbackState>,
    execution_result_cache: Mutex<ExecutionResultCacheState>,
    fulltext_index_options: Mutex<HashMap<FulltextIndexOptionsCacheKey, FulltextIndexOptions>>,
    started_at: Mutex<Option<Instant>>,
    schema_epochs: SchemaEpochTracker,
    schema_epoch: AtomicU64,
    data_epoch: AtomicU64,
    index_feedback_epoch: AtomicU64,
}

pub struct PgwireSessionGuard {
    runtime: Arc<RuntimeState>,
}

impl Drop for PgwireSessionGuard {
    fn drop(&mut self) {
        self.runtime.finish_pgwire_session();
    }
}

impl RuntimeState {
    #[must_use]
    pub fn new(limits: CassieRuntimeLimits) -> Self {
        let mut metrics = RuntimeMetricsState::default();
        metrics.plan_cache.max_entries = limits.plan_cache_entries as u64;
        metrics.feedback.max_entries = limits.feedback_entries as u64;
        Self {
            limits,
            metrics: Mutex::new(metrics),
            plan_cache: Mutex::new(PlanCacheState::default()),
            feedback: Mutex::new(RuntimeFeedbackState::default()),
            execution_result_cache: Mutex::new(ExecutionResultCacheState::default()),
            fulltext_index_options: Mutex::new(HashMap::new()),
            started_at: Mutex::new(None),
            schema_epochs: SchemaEpochTracker::default(),
            schema_epoch: AtomicU64::new(0),
            data_epoch: AtomicU64::new(0),
            index_feedback_epoch: AtomicU64::new(0),
        }
    }

    pub fn limits(&self) -> CassieRuntimeLimits {
        self.limits.clone()
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn mark_started(&self) {
        let mut started_at = self.started_at.lock().expect("runtime clock");
        if started_at.is_none() {
            *started_at = Some(Instant::now());
        }
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn mark_shutdown(&self) {
        let mut started_at = self.started_at.lock().expect("runtime clock");
        *started_at = None;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_startup(&self, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.started = true;
        metrics.runtime.startup_total += 1;
        metrics.runtime.startup_ms_total += duration_ms(elapsed);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_shutdown(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.started = false;
        metrics.runtime.shutdown_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_catalog_hydration(&self, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.catalog_hydration_total += 1;
        metrics.runtime.catalog_hydration_ms_total += duration_ms(elapsed);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_sql_parse(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.sql_parse_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn begin_pgwire_session(self: &Arc<Self>) -> PgwireSessionGuard {
        {
            let mut metrics = self.metrics.lock().expect("runtime metrics");
            metrics.pgwire.active_sessions += 1;
            metrics.pgwire.sessions_started_total += 1;
        }

        PgwireSessionGuard {
            runtime: Arc::clone(self),
        }
    }

    fn finish_pgwire_session(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.active_sessions = metrics.pgwire.active_sessions.saturating_sub(1);
        metrics.pgwire.sessions_finished_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_success(&self, elapsed: Duration, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query.count += 1;
        metrics.query.latency_ms_total += duration_ms(elapsed);
        metrics.query.rows_returned_total += rows as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_error(&self, elapsed: Duration, error: &CassieError) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query.count += 1;
        metrics.query.latency_ms_total += duration_ms(elapsed);
        metrics.query.errors_total += 1;
        *metrics
            .query
            .errors_by_class
            .entry(error_class(error).to_string())
            .or_insert(0) += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rest_request(&self, method: &str, route: &str, status: u16, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.rest.requests_total += 1;
        metrics.rest.latency_ms_total += duration_ms(elapsed);
        *metrics
            .rest
            .by_method
            .entry(method.to_ascii_uppercase())
            .or_insert(0) += 1;
        *metrics.rest.by_route.entry(route.to_string()).or_insert(0) += 1;
        *metrics
            .rest
            .by_status_class
            .entry(status_class(status))
            .or_insert(0) += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_auth_ok(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.auth_ok_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_auth_failed(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.auth_failed_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_protocol_error(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.protocol_errors_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_simple_query(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.simple_queries_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_extended_query(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.extended_queries_total += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_prepared_delta(&self, delta: isize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        adjust_signed(&mut metrics.pgwire.prepared_statements, delta);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_portal_delta(&self, delta: isize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        adjust_signed(&mut metrics.pgwire.portals, delta);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_message(&self, message: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        *metrics
            .pgwire
            .messages_total
            .entry(message.to_ascii_lowercase())
            .or_insert(0) += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_boundary_started(&self, operation: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.pgwire.blocking_started_total, operation);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_boundary_completed(&self, operation: &str, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.pgwire.blocking_completed_total, operation);
        increment_boundary_latency(
            &mut metrics.pgwire.blocking_elapsed_ms_total,
            operation,
            elapsed,
        );
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_boundary_error(&self, operation: &str, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.pgwire.blocking_error_total, operation);
        increment_boundary_latency(
            &mut metrics.pgwire.blocking_elapsed_ms_total,
            operation,
            elapsed,
        );
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_pgwire_boundary_join_failed(&self, operation: &str, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.pgwire.blocking_join_failed_total, operation);
        increment_boundary_latency(
            &mut metrics.pgwire.blocking_elapsed_ms_total,
            operation,
            elapsed,
        );
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rest_boundary_started(&self, operation: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.rest.blocking_started_total, operation);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rest_boundary_completed(&self, operation: &str, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.rest.blocking_completed_total, operation);
        increment_boundary_latency(
            &mut metrics.rest.blocking_elapsed_ms_total,
            operation,
            elapsed,
        );
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rest_boundary_error(&self, operation: &str, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.rest.blocking_error_total, operation);
        increment_boundary_latency(
            &mut metrics.rest.blocking_elapsed_ms_total,
            operation,
            elapsed,
        );
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rest_boundary_join_failed(&self, operation: &str, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.rest.blocking_join_failed_total, operation);
        increment_boundary_latency(
            &mut metrics.rest.blocking_elapsed_ms_total,
            operation,
            elapsed,
        );
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_search_execution(&self, elapsed: Duration, candidates: usize, results: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.search.count += 1;
        metrics.search.latency_ms_total += duration_ms(elapsed);
        metrics.search.candidate_count_total += candidates as u64;
        metrics.search.result_count_total += results as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_fulltext_row_scan_fallback(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.search.retrieval_stage_queries_total += 1;
        metrics.search.row_scan_fallback_total += 1;
        increment_boundary_counter(&mut metrics.search.retrieval_fallback_reasons, reason);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_vector_execution(&self, elapsed: Duration, candidates: usize, results: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.count += 1;
        metrics.vector.latency_ms_total += duration_ms(elapsed);
        metrics.vector.candidate_count_total += candidates as u64;
        metrics.vector.result_count_total += results as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_vector_normalization_usage(
        &self,
        normalized_candidates: usize,
        fallback_candidates: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.normalized_candidate_count_total += normalized_candidates as u64;
        metrics.vector.normalized_fallback_count_total += fallback_candidates as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_vector_prefilter_usage(
        &self,
        input_candidates: usize,
        filtered_candidates: usize,
        fallback_reason: Option<&str>,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.prefilter_input_candidate_count_total += input_candidates as u64;
        metrics.vector.prefilter_filtered_candidate_count_total += filtered_candidates as u64;
        if let Some(reason) = fallback_reason {
            metrics.vector.prefilter_fallback_count_total += 1;
            *metrics
                .vector
                .prefilter_fallback_reasons
                .entry(reason.to_string())
                .or_insert(0) += 1;
        }
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_hybrid_execution(&self, elapsed: Duration, candidates: usize, results: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.hybrid.count += 1;
        metrics.hybrid.latency_ms_total += duration_ms(elapsed);
        metrics.hybrid.candidate_count_total += candidates as u64;
        metrics.hybrid.result_count_total += results as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_hybrid_prefilter_usage(
        &self,
        input_candidates: usize,
        filtered_candidates: usize,
        fallback_reason: Option<&str>,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.hybrid.prefilter_input_candidate_count_total += input_candidates as u64;
        metrics.hybrid.prefilter_filtered_candidate_count_total += filtered_candidates as u64;
        if let Some(reason) = fallback_reason {
            metrics.hybrid.prefilter_fallback_count_total += 1;
            *metrics
                .hybrid
                .prefilter_fallback_reasons
                .entry(reason.to_string())
                .or_insert(0) += 1;
        }
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_storage_access(&self, family: &str, write: bool, success: bool) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        let family = family.to_ascii_lowercase();
        let counters = match family.as_str() {
            "schema" => &mut metrics.storage.schema,
            "data" => &mut metrics.storage.data,
            "temp" => &mut metrics.storage.temp,
            _ => &mut metrics.storage.default_family,
        };

        if write {
            counters.writes += 1;
        } else {
            counters.reads += 1;
        }

        if !success {
            counters.errors += 1;
        }
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_plan_cache_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.hits += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_plan_cache_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.misses += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_l1_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l1_hits += 1;
        metrics.plan_cache.hits += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_l1_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l1_misses += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_l2_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l2_hits += 1;
        metrics.plan_cache.hits += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_l2_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l2_misses += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_compile_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.misses += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_promotion(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.candidate_promotions += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_schema_epoch_reject(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.schema_epoch_rejects += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_deserialize_reject(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.deserialize_rejects += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_fulltext_stats_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.fulltext_stats_hits += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_query_cache_fulltext_stats_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.fulltext_stats_misses += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_cardinality_read(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.cardinality.reads += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_cardinality_write(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.cardinality.writes += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_cardinality_rebuild(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.cardinality.rebuilds += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_cardinality_unavailable(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.cardinality.unavailable += 1;
    }

    fn record_feedback_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.feedback.hits += 1;
    }

    fn record_feedback_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.feedback.misses += 1;
    }

    fn record_feedback_write(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.feedback.writes += 1;
    }

    fn record_feedback_eviction(&self, evictions: u64) {
        if evictions == 0 {
            return;
        }

        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.feedback.evictions += evictions;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_covering_index_scan(&self, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.covering_indexes.scans += 1;
        metrics.covering_indexes.row_fetches_avoided += rows as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_covering_index_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.covering_indexes.fallback_scans += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_aggregate_acceleration(&self, accelerated_segments: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.aggregate_acceleration.scans += 1;
        metrics.aggregate_acceleration.accelerated_segments += accelerated_segments as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_aggregate_acceleration_row_blob_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.aggregate_acceleration.row_blob_fallbacks += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_parallel_scan(&self, workers: usize, shards: usize, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scans.scans += 1;
        metrics.parallel_scans.workers += workers as u64;
        metrics.parallel_scans.shards += shards as u64;
        metrics.parallel_scans.rows += rows as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_parallel_scan_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scans.fallback_scans += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_parallel_scoring(&self, workers: usize, partitions: usize, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scoring.scorings += 1;
        metrics.parallel_scoring.workers += workers as u64;
        metrics.parallel_scoring.partitions += partitions as u64;
        metrics.parallel_scoring.rows += rows as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_parallel_scoring_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scoring.fallback_scorings += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_parallel_aggregation(
        &self,
        workers: usize,
        partitions: usize,
        rows: usize,
        groups: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_aggregation.aggregations += 1;
        metrics.parallel_aggregation.workers += workers as u64;
        metrics.parallel_aggregation.partitions += partitions as u64;
        metrics.parallel_aggregation.rows += rows as u64;
        metrics.parallel_aggregation.groups += groups as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_parallel_aggregation_fallback(&self, reason: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_aggregation.fallback_aggregations += 1;
        metrics.parallel_aggregation.last_fallback_reason = reason.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_plan_cache_invalidation(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.invalidations += 1;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_plan_cache_eviction(&self, evictions: u64) {
        if evictions == 0 {
            return;
        }

        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.evictions += evictions;
    }

    pub fn schema_epoch(&self) -> u64 {
        self.schema_epoch.load(Ordering::SeqCst)
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn set_schema_epoch(&self, schema_epoch: u64) {
        self.schema_epoch.store(schema_epoch, Ordering::SeqCst);
        self.fulltext_index_options
            .lock()
            .expect("fulltext index options")
            .clear();
        self.clear_feedback();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn fulltext_index_options_lookup(
        &self,
        key: &FulltextIndexOptionsCacheKey,
    ) -> Option<FulltextIndexOptions> {
        self.fulltext_index_options
            .lock()
            .expect("fulltext index options")
            .get(key)
            .cloned()
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn store_fulltext_index_options(
        &self,
        key: FulltextIndexOptionsCacheKey,
        options: FulltextIndexOptions,
    ) {
        self.fulltext_index_options
            .lock()
            .expect("fulltext index options")
            .insert(key, options);
    }
}

fn increment_boundary_counter(map: &mut BTreeMap<String, u64>, operation: &str) {
    *map.entry(operation.to_ascii_lowercase()).or_insert(0) += 1;
}

fn increment_boundary_latency(map: &mut BTreeMap<String, u64>, operation: &str, elapsed: Duration) {
    let bucket = map.entry(operation.to_ascii_lowercase()).or_insert(0);
    *bucket = bucket.saturating_add(duration_ms(elapsed));
}
