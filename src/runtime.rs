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
use crate::types::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionMode {
    SimpleQuery,
    DescribeQuery,
    ExtendedQuery,
}

#[derive(Debug, Clone, Copy)]
pub struct QueryExecutionControls {
    pub deadline: Option<Instant>,
    pub max_result_rows: usize,
    pub temp_spill_budget_bytes: usize,
    pub cte_recursion_depth: usize,
}

impl QueryExecutionControls {
    pub fn from_limits(limits: &CassieRuntimeLimits, started_at: Instant) -> Self {
        let deadline = if limits.query_timeout_ms == 0 {
            Some(started_at)
        } else {
            started_at
                .checked_add(Duration::from_millis(limits.query_timeout_ms))
                .or(Some(started_at))
        };

        Self {
            deadline,
            max_result_rows: limits.max_result_rows,
            temp_spill_budget_bytes: limits.temp_spill_budget_bytes,
            cte_recursion_depth: limits.cte_recursion_depth,
        }
    }

    pub fn is_timed_out(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeFeedbackKey {
    pub sql_fingerprint: u64,
    pub schema_epoch: u64,
    pub database: Option<String>,
    pub collection: String,
    pub operator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeFeedbackRecord {
    pub executions: u64,
    pub rows_in_total: u64,
    pub rows_out_total: u64,
    pub elapsed_ms_total: u64,
    pub storage_reads_total: u64,
    pub storage_writes_total: u64,
    pub temp_writes_total: u64,
    pub candidate_count_total: u64,
    pub result_count_total: u64,
    pub errors_total: u64,
    pub last_error_class: Option<String>,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeFeedbackObservation {
    pub rows_in: u64,
    pub rows_out: u64,
    pub elapsed_ms: u64,
    pub storage_reads: u64,
    pub storage_writes: u64,
    pub temp_writes: u64,
    pub candidate_count: u64,
    pub result_count: u64,
    pub error_class: Option<String>,
}

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

#[derive(Debug, Clone, Default)]
pub struct FulltextIndexOptions {
    pub field_boost: HashMap<String, f64>,
    pub field_k1: HashMap<String, f64>,
    pub field_b: HashMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FulltextIndexOptionsCacheKey {
    pub schema_epoch: u64,
    pub collection: String,
    pub fields: Vec<String>,
}

impl FulltextIndexOptionsCacheKey {
    pub fn new<I>(schema_epoch: u64, collection: &str, fields: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let mut normalized_fields = fields
            .into_iter()
            .map(|field| field.to_ascii_lowercase())
            .collect::<Vec<_>>();
        normalized_fields.sort();
        normalized_fields.dedup();

        Self {
            schema_epoch,
            collection: collection.to_ascii_lowercase(),
            fields: normalized_fields,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RuntimeSnapshot {
    pub started: bool,
    pub uptime_seconds: u64,
    pub running_queries: u64,
    pub sql_parse_total: u64,
    pub startup_total: u64,
    pub startup_ms_total: u64,
    pub shutdown_total: u64,
    pub catalog_hydration_total: u64,
    pub catalog_hydration_ms_total: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct QuerySnapshot {
    pub count: u64,
    pub latency_ms_total: u64,
    pub rows_returned_total: u64,
    pub errors_total: u64,
    pub errors_by_class: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RestSnapshot {
    pub requests_total: u64,
    pub latency_ms_total: u64,
    pub by_method: BTreeMap<String, u64>,
    pub by_route: BTreeMap<String, u64>,
    pub by_status_class: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PgwireSnapshot {
    pub active_sessions: u64,
    pub sessions_started_total: u64,
    pub sessions_finished_total: u64,
    pub auth_ok_total: u64,
    pub auth_failed_total: u64,
    pub protocol_errors_total: u64,
    pub simple_queries_total: u64,
    pub extended_queries_total: u64,
    pub prepared_statements: u64,
    pub portals: u64,
    pub messages_total: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ExecutionSnapshot {
    pub count: u64,
    pub latency_ms_total: u64,
    pub candidate_count_total: u64,
    pub result_count_total: u64,
    pub normalized_candidate_count_total: u64,
    pub normalized_fallback_count_total: u64,
    pub prefilter_input_candidate_count_total: u64,
    pub prefilter_filtered_candidate_count_total: u64,
    pub prefilter_fallback_count_total: u64,
    pub prefilter_fallback_reasons: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PlanCacheSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub invalidations: u64,
    pub evictions: u64,
    pub entries: u64,
    pub max_entries: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct QueryCacheSnapshot {
    pub l1_hits: u64,
    pub l1_misses: u64,
    pub l2_hits: u64,
    pub l2_misses: u64,
    pub candidate_promotions: u64,
    pub schema_epoch_rejects: u64,
    pub deserialize_rejects: u64,
    pub fulltext_stats_hits: u64,
    pub fulltext_stats_misses: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CardinalitySnapshot {
    pub reads: u64,
    pub writes: u64,
    pub rebuilds: u64,
    pub unavailable: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FeedbackSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub writes: u64,
    pub evictions: u64,
    pub entries: u64,
    pub max_entries: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StorageFamilySnapshot {
    pub reads: u64,
    pub writes: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StorageSnapshot {
    pub schema: StorageFamilySnapshot,
    pub data: StorageFamilySnapshot,
    pub temp: StorageFamilySnapshot,
    #[serde(rename = "default")]
    pub default_family: StorageFamilySnapshot,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RuntimeMetricsSnapshot {
    pub runtime: RuntimeSnapshot,
    pub query: QuerySnapshot,
    pub rest: RestSnapshot,
    pub pgwire: PgwireSnapshot,
    pub search: ExecutionSnapshot,
    pub vector: ExecutionSnapshot,
    pub hybrid: ExecutionSnapshot,
    pub storage: StorageSnapshot,
    pub plan_cache: PlanCacheSnapshot,
    pub query_cache: QueryCacheSnapshot,
    pub cardinality: CardinalitySnapshot,
    pub feedback: FeedbackSnapshot,
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

pub fn hash_params(params: &[Value]) -> u64 {
    use std::hash::Hasher;
    fn hash_value(hasher: &mut std::hash::DefaultHasher, value: &Value) {
        match value {
            Value::Null => 0u8.hash(hasher),
            Value::Bool(v) => {
                1u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Int64(v) => {
                2u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Float64(v) => {
                3u8.hash(hasher);
                v.to_bits().hash(hasher);
            }
            Value::String(v) => {
                4u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Vector(v) => {
                5u8.hash(hasher);
                v.values.len().hash(hasher);
            }
            Value::Json(v) => {
                6u8.hash(hasher);
                v.to_string().hash(hasher);
            }
        }
    }
    let mut hasher = std::hash::DefaultHasher::new();
    for param in params {
        hash_value(&mut hasher, param);
    }
    hasher.finish()
}

pub fn parameter_shape(params: &[Value]) -> Vec<ParameterShape> {
    params.iter().map(parameter_shape_for_value).collect()
}

pub fn sql_fingerprint(statement: &crate::sql::ast::ParsedStatement) -> u64 {
    stable_fingerprint(&statement.statement)
}

pub fn error_class(error: &CassieError) -> &'static str {
    match error {
        CassieError::CollectionNotFound(_) => "collection_not_found",
        CassieError::Parse(_) => "parse",
        CassieError::Planner(_) => "planner",
        CassieError::Execution(_) => "execution",
        CassieError::InvalidVector(_) => "invalid_vector",
        CassieError::InvalidEmbedding(_) => "invalid_embedding",
        CassieError::EmbeddingUnavailable(_) => "embedding_unavailable",
        CassieError::Unauthorized => "unauthorized",
        CassieError::NotFound(_) => "not_found",
        CassieError::Unsupported(_) => "unsupported",
        CassieError::Storage(_) => "storage",
        CassieError::StorageBootstrap(_) => "storage_bootstrap",
        CassieError::StorageMissingFamily(_) => "storage_missing_family",
        CassieError::StorageRetryable(_) => "storage_retryable",
    }
}

fn parameter_shape_for_value(value: &Value) -> ParameterShape {
    match value {
        Value::Null => ParameterShape::Null,
        Value::Bool(_) => ParameterShape::Bool,
        Value::Int64(_) => ParameterShape::Int64,
        Value::Float64(_) => ParameterShape::Float64,
        Value::String(_) => ParameterShape::String,
        Value::Vector(vector) => ParameterShape::Vector(vector.values.len()),
        Value::Json(_) => ParameterShape::Json,
    }
}

fn status_class(status: u16) -> String {
    let class = status / 100;
    format!("{class}xx")
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn adjust_signed(value: &mut u64, delta: isize) {
    if delta.is_negative() {
        *value = value.saturating_sub(delta.unsigned_abs() as u64);
    } else {
        *value = value.saturating_add(delta as u64);
    }
}

fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn stable_fingerprint<T: Serialize>(value: &T) -> u64 {
    let mut writer = StableFingerprintWriter::default();
    serde_json::to_writer(&mut writer, value).expect("serialize stable fingerprint");
    writer.finish()
}

#[derive(Default)]
struct StableFingerprintWriter {
    state: u64,
}

impl StableFingerprintWriter {
    fn finish(&self) -> u64 {
        self.state
    }
}

impl Write for StableFingerprintWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        if self.state == 0 {
            self.state = FNV_OFFSET_BASIS;
        }
        for byte in buf {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn touch(order: &mut VecDeque<PlanCacheKey>, key: &PlanCacheKey) {
    if let Some(position) = order.iter().position(|entry| entry == key) {
        order.remove(position);
    }
    order.push_back(key.clone());
}

fn touch_feedback(order: &mut VecDeque<RuntimeFeedbackKey>, key: &RuntimeFeedbackKey) {
    if let Some(position) = order.iter().position(|entry| entry == key) {
        order.remove(position);
    }
    order.push_back(key.clone());
}

fn apply_feedback_observation(
    record: &mut RuntimeFeedbackRecord,
    observation: &RuntimeFeedbackObservation,
    now_ms: u64,
) {
    record.executions = record.executions.saturating_add(1);
    record.rows_in_total = record.rows_in_total.saturating_add(observation.rows_in);
    record.rows_out_total = record.rows_out_total.saturating_add(observation.rows_out);
    record.elapsed_ms_total = record
        .elapsed_ms_total
        .saturating_add(observation.elapsed_ms);
    record.storage_reads_total = record
        .storage_reads_total
        .saturating_add(observation.storage_reads);
    record.storage_writes_total = record
        .storage_writes_total
        .saturating_add(observation.storage_writes);
    record.temp_writes_total = record
        .temp_writes_total
        .saturating_add(observation.temp_writes);
    record.candidate_count_total = record
        .candidate_count_total
        .saturating_add(observation.candidate_count);
    record.result_count_total = record
        .result_count_total
        .saturating_add(observation.result_count);
    if let Some(error_class) = observation.error_class.as_ref() {
        record.errors_total = record.errors_total.saturating_add(1);
        record.last_error_class = Some(error_class.clone());
    }
    if record.first_seen_ms == 0 {
        record.first_seen_ms = now_ms;
    }
    record.last_seen_ms = now_ms;
}

fn prune_feedback_by_age(
    feedback: &mut RuntimeFeedbackState,
    now_ms: u64,
    ttl_seconds: u64,
) -> u64 {
    if ttl_seconds == 0 {
        return 0;
    }

    let ttl_ms = ttl_seconds.saturating_mul(1_000);
    let mut evictions = 0;
    let expired = feedback
        .entries
        .iter()
        .filter_map(|(key, record)| {
            (now_ms.saturating_sub(record.last_seen_ms) > ttl_ms).then(|| key.clone())
        })
        .collect::<Vec<_>>();

    for key in expired {
        if feedback.entries.remove(&key).is_some() {
            evictions += 1;
        }
        if let Some(position) = feedback.order.iter().position(|entry| entry == &key) {
            feedback.order.remove(position);
        }
    }

    evictions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::logical::LogicalPlan;
    use crate::planner::physical::{Operator, PhysicalPlan};
    use crate::sql::ast::QuerySource;

    fn sample_plan() -> PhysicalPlan {
        PhysicalPlan {
            collection: "bench_documents".to_string(),
            operators: vec![Operator::Scan, Operator::Filter, Operator::Project],
            predicate_pushdown: false,
            projected_scan_fields: Vec::new(),
            scan_limit: None,
            selected_index: None,
            top_k: false,
            top_k_limit: None,
            join_strategy: None,
            logical: LogicalPlan {
                command: None,
                source: QuerySource::Collection("bench_documents".to_string()),
                collection: "bench_documents".to_string(),
                ctes: Vec::new(),
                distinct: false,
                distinct_on: Vec::new(),
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: Some(20),
                offset: None,
                set: None,
            },
        }
    }

    #[test]
    fn should_reuse_cached_plan_arc_on_lookup() {
        // Arrange
        let runtime = RuntimeState::new(crate::config::CassieRuntimeLimits::default());
        let key = PlanCacheKey {
            sql_fingerprint: 42,
            schema_epoch: 1,
            parameter_shape: vec![ParameterShape::Int64],
            mode: ExecutionMode::SimpleQuery,
            database: Some("postgres".to_string()),
        };
        runtime.plan_cache_store(key.clone(), Arc::new(sample_plan()), false);

        // Act
        let first = runtime.plan_cache_lookup(&key).expect("cached plan");
        let second = runtime.plan_cache_lookup(&key).expect("cached plan");

        // Assert
        assert!(Arc::ptr_eq(&first.plan, &second.plan));
    }
}
