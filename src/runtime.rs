use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::Hash;
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
    pub parameter_shape: Vec<ParameterShape>,
    pub mode: ExecutionMode,
    pub database: Option<String>,
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
    pub mode: ExecutionMode,
}

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
#[path = "runtime/projection_metrics.rs"]
mod projection_metrics;
#[path = "runtime/query_cache.rs"]
pub(crate) mod query_cache;
#[path = "runtime/retention_metrics.rs"]
mod retention_metrics;
#[path = "runtime/rollup_metrics.rs"]
mod rollup_metrics;
#[path = "runtime/snapshots.rs"]
mod snapshots;

pub use controls::QueryExecutionControls;
pub use feedback::{RuntimeFeedbackKey, RuntimeFeedbackObservation, RuntimeFeedbackRecord};
pub use fulltext::{FulltextIndexOptions, FulltextIndexOptionsCacheKey};
pub(crate) use helpers::stable_fingerprint;
use helpers::*;
pub use helpers::{error_class, hash_params, parameter_shape, sql_fingerprint};
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
    covering_indexes: CoveringIndexSnapshot,
    column_batches: ColumnBatchSnapshot,
    aggregate_acceleration: AggregateAccelerationSnapshot,
    parallel_scans: ParallelScanSnapshot,
    parallel_scoring: ParallelScoringSnapshot,
    parallel_aggregation: ParallelAggregationSnapshot,
    rollups: RollupSnapshot,
    projections: ProjectionSnapshot,
    retention: RetentionSnapshot,
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
    schema_epoch: AtomicU64,
    data_epoch: AtomicU64,
}

pub struct RunningQueryGuard {
    runtime: Arc<RuntimeState>,
}

pub struct PgwireSessionGuard {
    runtime: Arc<RuntimeState>,
}

impl Drop for RunningQueryGuard {
    fn drop(&mut self) {
        self.runtime.finish_running_query();
    }
}

impl Drop for PgwireSessionGuard {
    fn drop(&mut self) {
        self.runtime.finish_pgwire_session();
    }
}

impl RuntimeState {
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
            schema_epoch: AtomicU64::new(0),
            data_epoch: AtomicU64::new(0),
        }
    }

    pub fn limits(&self) -> CassieRuntimeLimits {
        self.limits.clone()
    }

    pub fn mark_started(&self) {
        let mut started_at = self.started_at.lock().expect("runtime clock");
        if started_at.is_none() {
            *started_at = Some(Instant::now());
        }
    }

    pub fn mark_shutdown(&self) {
        let mut started_at = self.started_at.lock().expect("runtime clock");
        *started_at = None;
    }

    pub fn record_startup(&self, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.started = true;
        metrics.runtime.startup_total += 1;
        metrics.runtime.startup_ms_total += duration_ms(elapsed);
    }

    pub fn record_shutdown(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.started = false;
        metrics.runtime.shutdown_total += 1;
    }

    pub fn record_catalog_hydration(&self, elapsed: Duration) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.catalog_hydration_total += 1;
        metrics.runtime.catalog_hydration_ms_total += duration_ms(elapsed);
    }

    pub fn record_sql_parse(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.sql_parse_total += 1;
    }

    pub fn begin_running_query(self: &Arc<Self>) -> RunningQueryGuard {
        {
            let mut metrics = self.metrics.lock().expect("runtime metrics");
            metrics.runtime.running_queries += 1;
        }

        RunningQueryGuard {
            runtime: Arc::clone(self),
        }
    }

    fn finish_running_query(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.running_queries = metrics.runtime.running_queries.saturating_sub(1);
    }

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

    pub fn record_query_success(&self, elapsed: Duration, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query.count += 1;
        metrics.query.latency_ms_total += duration_ms(elapsed);
        metrics.query.rows_returned_total += rows as u64;
    }

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

    pub fn record_pgwire_auth_ok(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.auth_ok_total += 1;
    }

    pub fn record_pgwire_auth_failed(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.auth_failed_total += 1;
    }

    pub fn record_pgwire_protocol_error(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.protocol_errors_total += 1;
    }

    pub fn record_pgwire_simple_query(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.simple_queries_total += 1;
    }

    pub fn record_pgwire_extended_query(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.pgwire.extended_queries_total += 1;
    }

    pub fn record_pgwire_prepared_delta(&self, delta: isize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        adjust_signed(&mut metrics.pgwire.prepared_statements, delta);
    }

    pub fn record_pgwire_portal_delta(&self, delta: isize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        adjust_signed(&mut metrics.pgwire.portals, delta);
    }

    pub fn record_pgwire_message(&self, message: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        *metrics
            .pgwire
            .messages_total
            .entry(message.to_ascii_lowercase())
            .or_insert(0) += 1;
    }

    pub fn record_search_execution(&self, elapsed: Duration, candidates: usize, results: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.search.count += 1;
        metrics.search.latency_ms_total += duration_ms(elapsed);
        metrics.search.candidate_count_total += candidates as u64;
        metrics.search.result_count_total += results as u64;
    }

    pub fn record_vector_execution(&self, elapsed: Duration, candidates: usize, results: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.count += 1;
        metrics.vector.latency_ms_total += duration_ms(elapsed);
        metrics.vector.candidate_count_total += candidates as u64;
        metrics.vector.result_count_total += results as u64;
    }

    pub fn record_vector_normalization_usage(
        &self,
        normalized_candidates: usize,
        fallback_candidates: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.normalized_candidate_count_total += normalized_candidates as u64;
        metrics.vector.normalized_fallback_count_total += fallback_candidates as u64;
    }

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

    pub fn record_hybrid_execution(&self, elapsed: Duration, candidates: usize, results: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.hybrid.count += 1;
        metrics.hybrid.latency_ms_total += duration_ms(elapsed);
        metrics.hybrid.candidate_count_total += candidates as u64;
        metrics.hybrid.result_count_total += results as u64;
    }

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

    pub fn record_plan_cache_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.hits += 1;
    }

    pub fn record_plan_cache_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.misses += 1;
    }

    pub fn record_query_cache_l1_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l1_hits += 1;
        metrics.plan_cache.hits += 1;
    }

    pub fn record_query_cache_l1_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l1_misses += 1;
    }

    pub fn record_query_cache_l2_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l2_hits += 1;
        metrics.plan_cache.hits += 1;
    }

    pub fn record_query_cache_l2_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.l2_misses += 1;
    }

    pub fn record_query_cache_compile_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.misses += 1;
    }

    pub fn record_query_cache_promotion(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.candidate_promotions += 1;
    }

    pub fn record_query_cache_schema_epoch_reject(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.schema_epoch_rejects += 1;
    }

    pub fn record_query_cache_deserialize_reject(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.deserialize_rejects += 1;
    }

    pub fn record_query_cache_fulltext_stats_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.fulltext_stats_hits += 1;
    }

    pub fn record_query_cache_fulltext_stats_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.query_cache.fulltext_stats_misses += 1;
    }

    pub fn record_cardinality_read(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.cardinality.reads += 1;
    }

    pub fn record_cardinality_write(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.cardinality.writes += 1;
    }

    pub fn record_cardinality_rebuild(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.cardinality.rebuilds += 1;
    }

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

    pub fn feedback_lookup(&self, key: &RuntimeFeedbackKey) -> Option<RuntimeFeedbackRecord> {
        let now_ms = current_time_millis();
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        let evictions =
            prune_feedback_by_age(&mut feedback, now_ms, self.limits.feedback_ttl_seconds);
        let record = feedback.entries.get(key).cloned();
        if record.is_some() {
            touch_feedback(&mut feedback.order, key);
        }
        drop(feedback);

        self.record_feedback_eviction(evictions);
        if record.is_some() {
            self.record_feedback_hit();
        } else {
            self.record_feedback_miss();
        }
        record
    }

    pub fn feedback_record(&self, key: &RuntimeFeedbackKey) -> Option<RuntimeFeedbackRecord> {
        self.feedback
            .lock()
            .expect("runtime feedback")
            .entries
            .get(key)
            .cloned()
    }

    pub fn record_feedback(
        &self,
        key: RuntimeFeedbackKey,
        observation: RuntimeFeedbackObservation,
    ) {
        let now_ms = current_time_millis();
        let max_entries = self.limits.feedback_entries.max(1);
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        let mut evictions =
            prune_feedback_by_age(&mut feedback, now_ms, self.limits.feedback_ttl_seconds);

        if let Some(record) = feedback.entries.get_mut(&key) {
            apply_feedback_observation(record, &observation, now_ms);
            touch_feedback(&mut feedback.order, &key);
        } else {
            while feedback.entries.len() >= max_entries {
                let Some(oldest) = feedback.order.pop_front() else {
                    break;
                };
                if feedback.entries.remove(&oldest).is_some() {
                    evictions += 1;
                }
            }

            let mut record = RuntimeFeedbackRecord {
                first_seen_ms: now_ms,
                last_seen_ms: now_ms,
                ..RuntimeFeedbackRecord::default()
            };
            apply_feedback_observation(&mut record, &observation, now_ms);
            feedback.entries.insert(key.clone(), record);
            feedback.order.push_back(key);
        }

        drop(feedback);
        self.record_feedback_write();
        self.record_feedback_eviction(evictions);
    }

    pub fn feedback_candidate_budget(&self, collection: &str) -> Option<usize> {
        let now_ms = current_time_millis();
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        let evictions =
            prune_feedback_by_age(&mut feedback, now_ms, self.limits.feedback_ttl_seconds);
        let budget = feedback
            .entries
            .iter()
            .filter(|(key, record)| {
                key.collection.eq_ignore_ascii_case(collection)
                    && record.executions > 0
                    && record.candidate_count_total > 0
            })
            .map(|(_, record)| {
                record
                    .candidate_count_total
                    .saturating_add(record.executions - 1)
                    / record.executions
            })
            .max()
            .and_then(|value| usize::try_from(value).ok());
        drop(feedback);
        self.record_feedback_eviction(evictions);
        budget
    }

    pub fn record_adaptive_candidate_decision(
        &self,
        initial_budget: usize,
        feedback_budget: Option<usize>,
        expansions: usize,
        final_candidate_count: usize,
        exhausted: bool,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.adaptive_candidates.decisions += 1;
        metrics.adaptive_candidates.initial_budget_total += initial_budget as u64;
        metrics.adaptive_candidates.feedback_budget_total +=
            feedback_budget.unwrap_or_default() as u64;
        metrics.adaptive_candidates.expansions_total += expansions as u64;
        metrics.adaptive_candidates.final_candidate_count_total += final_candidate_count as u64;
        if exhausted {
            metrics.adaptive_candidates.exhausted_total += 1;
        }
    }

    pub fn record_adaptive_candidate_limit_error(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.adaptive_candidates.limit_errors_total += 1;
    }

    pub fn record_covering_index_scan(&self, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.covering_indexes.scans += 1;
        metrics.covering_indexes.row_fetches_avoided += rows as u64;
    }

    pub fn record_covering_index_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.covering_indexes.fallback_scans += 1;
    }

    pub fn record_aggregate_acceleration(&self, accelerated_segments: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.aggregate_acceleration.scans += 1;
        metrics.aggregate_acceleration.accelerated_segments += accelerated_segments as u64;
    }

    pub fn record_aggregate_acceleration_row_blob_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.aggregate_acceleration.row_blob_fallbacks += 1;
    }

    pub fn record_parallel_scan(&self, workers: usize, shards: usize, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scans.scans += 1;
        metrics.parallel_scans.workers += workers as u64;
        metrics.parallel_scans.shards += shards as u64;
        metrics.parallel_scans.rows += rows as u64;
    }

    pub fn record_parallel_scan_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scans.fallback_scans += 1;
    }

    pub fn record_parallel_scoring(&self, workers: usize, partitions: usize, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scoring.scorings += 1;
        metrics.parallel_scoring.workers += workers as u64;
        metrics.parallel_scoring.partitions += partitions as u64;
        metrics.parallel_scoring.rows += rows as u64;
    }

    pub fn record_parallel_scoring_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_scoring.fallback_scorings += 1;
    }

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

    pub fn record_parallel_aggregation_fallback(&self, reason: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.parallel_aggregation.fallback_aggregations += 1;
        metrics.parallel_aggregation.last_fallback_reason = reason.into();
    }

    pub fn record_plan_cache_invalidation(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.invalidations += 1;
    }

    pub fn record_plan_cache_eviction(&self, evictions: u64) {
        if evictions == 0 {
            return;
        }

        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.plan_cache.evictions += evictions;
    }

    pub fn plan_cache_lookup(&self, key: &PlanCacheKey) -> Option<L1PlanHit> {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        if let Some(plan) = cache.entries.get(key).cloned() {
            touch(&mut cache.order, key);
            drop(cache);
            self.record_query_cache_l1_hit();
            return Some(L1PlanHit {
                plan: plan.plan,
                durable: plan.durable,
                candidate_expires_at_ms: plan.candidate_expires_at_ms,
            });
        }

        drop(cache);
        self.record_query_cache_l1_miss();
        None
    }

    pub fn plan_cache_store(&self, key: PlanCacheKey, plan: Arc<PhysicalPlan>, durable: bool) {
        let max_entries = self.limits.plan_cache_entries.max(1);
        let mut cache = self.plan_cache.lock().expect("plan cache");
        let mut evictions = 0;
        let entry = L1PlanEntry {
            plan,
            durable,
            candidate_expires_at_ms: None,
        };

        if cache.entries.contains_key(&key) {
            cache.entries.insert(key.clone(), entry);
            touch(&mut cache.order, &key);
        } else {
            if cache.entries.len() >= max_entries {
                if let Some(oldest) = cache.order.pop_front() {
                    if cache.entries.remove(&oldest).is_some() {
                        evictions += 1;
                    }
                }
            }

            cache.entries.insert(key.clone(), entry);
            cache.order.push_back(key);
        }

        drop(cache);
        self.record_plan_cache_eviction(evictions);
    }

    pub fn mark_plan_cache_entry_durable(&self, key: &PlanCacheKey) {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        if let Some(entry) = cache.entries.get_mut(key) {
            entry.durable = true;
            entry.candidate_expires_at_ms = None;
        }
    }

    pub fn mark_plan_cache_entry_candidate_pending(&self, key: &PlanCacheKey, ttl_seconds: u64) {
        if ttl_seconds == 0 {
            return;
        }

        let expires_at_ms = current_time_millis().saturating_add(ttl_seconds.saturating_mul(1000));
        let mut cache = self.plan_cache.lock().expect("plan cache");
        if let Some(entry) = cache.entries.get_mut(key) {
            if !entry.durable {
                entry.candidate_expires_at_ms = Some(expires_at_ms);
            }
        }
    }

    pub fn invalidate_plan_cache(&self) {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        cache.entries.clear();
        cache.order.clear();
        drop(cache);
        self.fulltext_index_options
            .lock()
            .expect("fulltext index options")
            .clear();
        self.record_plan_cache_invalidation();
    }

    pub fn execution_result_cache_lookup(
        &self,
        key: &ExecutionResultCacheKey,
    ) -> Option<QueryResult> {
        let mut cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        if let Some(result) = cache.entries.get(key).cloned() {
            Self::execution_result_cache_touch(&mut cache.order, key);
            drop(cache);
            return Some(result);
        }
        None
    }

    pub fn execution_result_cache_store(&self, key: ExecutionResultCacheKey, result: QueryResult) {
        const MAX_ENTRIES: usize = 64;
        let mut cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        if let std::collections::hash_map::Entry::Occupied(mut entry) =
            cache.entries.entry(key.clone())
        {
            entry.insert(result);
            return;
        }
        if cache.entries.len() >= MAX_ENTRIES {
            if let Some(oldest) = cache.order.pop_front() {
                cache.entries.remove(&oldest);
            }
        }
        cache.entries.insert(key.clone(), result);
        cache.order.push_back(key);
    }

    pub fn invalidate_execution_result_cache(&self) {
        let mut cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        cache.entries.clear();
        cache.order.clear();
    }

    pub fn data_epoch(&self) -> u64 {
        self.data_epoch.load(Ordering::SeqCst)
    }

    pub fn bump_data_epoch(&self) {
        let epoch = self
            .data_epoch
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        self.data_epoch.store(epoch, Ordering::SeqCst);
        self.invalidate_execution_result_cache();
    }

    fn execution_result_cache_touch(
        order: &mut VecDeque<ExecutionResultCacheKey>,
        key: &ExecutionResultCacheKey,
    ) {
        if let Some(position) = order.iter().position(|entry| entry == key) {
            order.remove(position);
        }
        order.push_back(key.clone());
    }

    pub fn plan_cache_entry_count(&self) -> usize {
        self.plan_cache.lock().expect("plan cache").entries.len()
    }

    pub fn schema_epoch(&self) -> u64 {
        self.schema_epoch.load(Ordering::SeqCst)
    }

    pub fn set_schema_epoch(&self, schema_epoch: u64) {
        self.schema_epoch.store(schema_epoch, Ordering::SeqCst);
        self.fulltext_index_options
            .lock()
            .expect("fulltext index options")
            .clear();
    }

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

    pub fn query_controls(&self, started_at: Instant) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(&self.limits, started_at)
    }

    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        let metrics = self.metrics.lock().expect("runtime metrics");
        let started_at = self.started_at.lock().expect("runtime clock");
        let uptime_seconds = started_at
            .as_ref()
            .map(|instant| instant.elapsed().as_secs())
            .unwrap_or(0);
        let mut snapshot = RuntimeMetricsSnapshot {
            runtime: metrics.runtime.clone(),
            query: metrics.query.clone(),
            rest: metrics.rest.clone(),
            pgwire: metrics.pgwire.clone(),
            search: metrics.search.clone(),
            vector: metrics.vector.clone(),
            hybrid: metrics.hybrid.clone(),
            storage: metrics.storage.clone(),
            plan_cache: metrics.plan_cache.clone(),
            query_cache: metrics.query_cache.clone(),
            cardinality: metrics.cardinality.clone(),
            feedback: metrics.feedback.clone(),
            adaptive_candidates: metrics.adaptive_candidates.clone(),
            covering_indexes: metrics.covering_indexes.clone(),
            column_batches: metrics.column_batches.clone(),
            aggregate_acceleration: metrics.aggregate_acceleration.clone(),
            parallel_scans: metrics.parallel_scans.clone(),
            parallel_scoring: metrics.parallel_scoring.clone(),
            parallel_aggregation: metrics.parallel_aggregation.clone(),
            rollups: metrics.rollups.clone(),
            projections: metrics.projections.clone(),
            retention: metrics.retention.clone(),
        };
        snapshot.runtime.uptime_seconds = uptime_seconds;
        snapshot.runtime.running_queries = metrics.runtime.running_queries;
        snapshot.plan_cache.entries = self.plan_cache_entry_count() as u64;
        snapshot.plan_cache.max_entries = self.limits.plan_cache_entries as u64;
        snapshot.feedback.entries = self.feedback_entry_count() as u64;
        snapshot.feedback.max_entries = self.limits.feedback_entries as u64;
        snapshot
    }

    pub fn feedback_entry_count(&self) -> usize {
        self.feedback
            .lock()
            .expect("runtime feedback")
            .entries
            .len()
    }
}

use std::hash::Hasher;
