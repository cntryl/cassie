use super::Cassie;

impl Cassie {
    #[must_use]
    pub fn health(&self) -> serde_json::Value {
        let recovery_required = self.check_storage_recovery_required().is_err();
        let ready = self.is_started() && !recovery_required;
        let status = if recovery_required {
            "recovery_required"
        } else if ready {
            "ok"
        } else {
            "starting"
        };
        serde_json::json!({
            "status": status,
            "ready": ready,
            "collections": self.catalog.registered_collection_count(),
            "version": env!("CARGO_PKG_VERSION")
        })
    }

    /// Forces every `GRAPH_TABLE` read onto the exact full read of its mapped tables instead
    /// of the lazy indexed expansion. Both return identical results; equivalence tests and
    /// operators use this to compare them.
    pub fn set_graph_exact_reads(&self, exact: bool) {
        self.runtime.set_graph_exact_reads(exact);
    }

    #[must_use]
    pub fn metrics(&self) -> serde_json::Value {
        let snapshot = self.runtime.snapshot();
        let mut column_batches = serde_json::to_value(snapshot.column_batches).unwrap_or_default();
        let column_batch_operational = self.midge.column_batch_operational_metrics();
        if let (Some(column_batches), Some(operational)) = (
            column_batches.as_object_mut(),
            column_batch_operational.as_object(),
        ) {
            column_batches.extend(operational.clone());
        }
        let mut search = serde_json::to_value(snapshot.search).unwrap_or_default();
        if let (Some(search), Some(maintenance)) = (
            search.as_object_mut(),
            self.midge.fulltext_maintenance_metrics().as_object(),
        ) {
            search.extend(maintenance.clone());
        }
        serde_json::json!({
            "uptime_seconds": snapshot.runtime.uptime_seconds,
            "running_queries": snapshot.runtime.running_queries,
            "ready": self.is_started(),
            "auth_user": &self.auth_user,
            "runtime": snapshot.runtime,
            "data_epoch": self.runtime.data_epoch(),
            "query": snapshot.query,
            "transactions": { "unique_waiters": self.transaction_unique_waits.waiting_count() },
            "rest": snapshot.rest,
            "pgwire": snapshot.pgwire,
            "search": search,
            "vector": snapshot.vector,
            "hybrid": snapshot.hybrid,
            "storage": snapshot.storage,
            "plan_cache": snapshot.plan_cache,
            "query_cache": snapshot.query_cache,
            "execution_result_cache": snapshot.execution_result_cache,
            "cardinality": snapshot.cardinality,
            "feedback": snapshot.feedback,
            "adaptive_candidates": snapshot.adaptive_candidates,
            "joins": snapshot.joins,
            "covering_indexes": snapshot.covering_indexes,
            "column_batches": column_batches,
            "time_series": snapshot.time_series,
            "aggregate_acceleration": snapshot.aggregate_acceleration,
            "parallel_scans": snapshot.parallel_scans,
            "parallel_scoring": snapshot.parallel_scoring,
            "parallel_aggregation": snapshot.parallel_aggregation,
            "rollups": snapshot.rollups,
            "projections": snapshot.projections,
            "retention": snapshot.retention,
            "read_paths": snapshot.read_paths,
            "capacity": self.midge.capacity_report(),
        })
    }
}
