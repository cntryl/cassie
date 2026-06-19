use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::planner::physical::PhysicalPlan;
use crate::types::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlanCacheKey {
    pub normalized_sql: String,
    pub catalog_version: u64,
    pub parameter_shape: Vec<String>,
    pub mode: ExecutionMode,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RuntimeSnapshot {
    pub started: bool,
    pub uptime_seconds: u64,
    pub running_queries: u64,
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
}

#[derive(Debug, Default)]
struct PlanCacheState {
    entries: HashMap<PlanCacheKey, Arc<PhysicalPlan>>,
    order: VecDeque<PlanCacheKey>,
}

#[derive(Debug)]
pub struct RuntimeState {
    limits: CassieRuntimeLimits,
    metrics: Mutex<RuntimeMetricsState>,
    plan_cache: Mutex<PlanCacheState>,
    started_at: Mutex<Option<Instant>>,
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
        Self {
            limits,
            metrics: Mutex::new(metrics),
            plan_cache: Mutex::new(PlanCacheState::default()),
            started_at: Mutex::new(None),
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

    pub fn record_hybrid_execution(&self, elapsed: Duration, candidates: usize, results: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.hybrid.count += 1;
        metrics.hybrid.latency_ms_total += duration_ms(elapsed);
        metrics.hybrid.candidate_count_total += candidates as u64;
        metrics.hybrid.result_count_total += results as u64;
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

    pub fn plan_cache_lookup(&self, key: &PlanCacheKey) -> Option<Arc<PhysicalPlan>> {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        if let Some(plan) = cache.entries.get(key).cloned() {
            touch(&mut cache.order, key);
            drop(cache);
            self.record_plan_cache_hit();
            return Some(plan);
        }

        drop(cache);
        self.record_plan_cache_miss();
        None
    }

    pub fn plan_cache_store(&self, key: PlanCacheKey, plan: Arc<PhysicalPlan>) {
        let max_entries = self.limits.plan_cache_entries.max(1);
        let mut cache = self.plan_cache.lock().expect("plan cache");
        let mut evictions = 0;

        if cache.entries.contains_key(&key) {
            cache.entries.insert(key.clone(), plan);
            touch(&mut cache.order, &key);
        } else {
            if cache.entries.len() >= max_entries {
                if let Some(oldest) = cache.order.pop_front() {
                    if cache.entries.remove(&oldest).is_some() {
                        evictions += 1;
                    }
                }
            }

            cache.entries.insert(key.clone(), plan);
            cache.order.push_back(key);
        }

        drop(cache);
        self.record_plan_cache_eviction(evictions);
    }

    pub fn invalidate_plan_cache(&self) {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        cache.entries.clear();
        cache.order.clear();
        drop(cache);
        self.record_plan_cache_invalidation();
    }

    pub fn plan_cache_entry_count(&self) -> usize {
        self.plan_cache.lock().expect("plan cache").entries.len()
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
        };
        snapshot.runtime.uptime_seconds = uptime_seconds;
        snapshot.runtime.running_queries = metrics.runtime.running_queries;
        snapshot.plan_cache.entries = self.plan_cache_entry_count() as u64;
        snapshot.plan_cache.max_entries = self.limits.plan_cache_entries as u64;
        snapshot
    }
}

pub fn parameter_shape(params: &[Value]) -> Vec<String> {
    params.iter().map(parameter_shape_for_value).collect()
}

pub fn normalized_sql(statement: &crate::sql::ast::ParsedStatement) -> String {
    format!("{:?}", statement.statement)
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

fn parameter_shape_for_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Int64(_) => "int64".to_string(),
        Value::Float64(_) => "float64".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Vector(vector) => format!("vector({})", vector.values.len()),
        Value::Json(_) => "json".to_string(),
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

fn touch(order: &mut VecDeque<PlanCacheKey>, key: &PlanCacheKey) {
    if let Some(position) = order.iter().position(|entry| entry == key) {
        order.remove(position);
    }
    order.push_back(key.clone());
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
            logical: LogicalPlan {
                command: None,
                source: QuerySource::Collection("bench_documents".to_string()),
                collection: "bench_documents".to_string(),
                ctes: Vec::new(),
                distinct: false,
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
            normalized_sql: "select".to_string(),
            catalog_version: 1,
            parameter_shape: vec!["int64".to_string()],
            mode: ExecutionMode::SimpleQuery,
        };
        runtime.plan_cache_store(key.clone(), Arc::new(sample_plan()));

        // Act
        let first = runtime.plan_cache_lookup(&key).expect("cached plan");
        let second = runtime.plan_cache_lookup(&key).expect("cached plan");

        // Assert
        assert!(Arc::ptr_eq(&first, &second));
    }
}
