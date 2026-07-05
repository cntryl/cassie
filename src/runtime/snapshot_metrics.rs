use std::time::Instant;

use super::{QueryExecutionControls, RuntimeMetricsSnapshot, RuntimeState};

impl RuntimeState {
    pub fn query_controls(&self, started_at: Instant) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(&self.limits, started_at)
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        let metrics = self.metrics.lock().expect("runtime metrics");
        let started_at = self.started_at.lock().expect("runtime clock");
        let uptime_seconds = started_at
            .as_ref()
            .map_or(0, |instant| instant.elapsed().as_secs());
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
            joins: metrics.joins.clone(),
            covering_indexes: metrics.covering_indexes.clone(),
            column_batches: metrics.column_batches.clone(),
            time_series: metrics.time_series.clone(),
            aggregate_acceleration: metrics.aggregate_acceleration.clone(),
            parallel_scans: metrics.parallel_scans.clone(),
            parallel_scoring: metrics.parallel_scoring.clone(),
            parallel_aggregation: metrics.parallel_aggregation.clone(),
            rollups: metrics.rollups.clone(),
            projections: metrics.projections.clone(),
            retention: metrics.retention.clone(),
            read_paths: metrics.read_paths.clone(),
            graph: metrics.graph.clone(),
        };
        snapshot.runtime.uptime_seconds = uptime_seconds;
        snapshot.runtime.running_queries = metrics.runtime.running_queries;
        snapshot.plan_cache.entries = self.plan_cache_entry_count() as u64;
        snapshot.plan_cache.max_entries = self.limits.plan_cache_entries as u64;
        snapshot.feedback.entries = self.feedback_entry_count() as u64;
        snapshot.feedback.max_entries = self.limits.feedback_entries as u64;
        snapshot
    }
}
