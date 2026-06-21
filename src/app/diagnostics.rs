use super::*;

impl Cassie {
    pub fn health(&self) -> serde_json::Value {
        let ready = self.is_started();
        let collections = self.midge.list_collections();
        serde_json::json!({
            "status": if ready { "ok" } else { "starting" },
            "ready": ready,
            "collections": collections.len(),
            "version": env!("CARGO_PKG_VERSION")
        })
    }

    pub fn metrics(&self) -> serde_json::Value {
        let snapshot = self.runtime.snapshot();
        serde_json::json!({
            "uptime_seconds": snapshot.runtime.uptime_seconds,
            "running_queries": snapshot.runtime.running_queries,
            "ready": self.is_started(),
            "auth_user": &self.auth_user,
            "runtime": snapshot.runtime,
            "query": snapshot.query,
            "rest": snapshot.rest,
            "pgwire": snapshot.pgwire,
            "search": snapshot.search,
            "vector": snapshot.vector,
            "hybrid": snapshot.hybrid,
            "storage": snapshot.storage,
            "plan_cache": snapshot.plan_cache,
            "query_cache": snapshot.query_cache,
            "cardinality": snapshot.cardinality,
            "feedback": snapshot.feedback,
            "adaptive_candidates": snapshot.adaptive_candidates,
            "covering_indexes": snapshot.covering_indexes,
            "column_batches": snapshot.column_batches,
            "parallel_scans": snapshot.parallel_scans,
            "parallel_scoring": snapshot.parallel_scoring,
            "parallel_aggregation": snapshot.parallel_aggregation,
        })
    }
}
