use super::*;

impl RuntimeState {
    pub fn record_projection_replay(
        &self,
        projection: impl Into<String>,
        applied: u64,
        skipped: u64,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.replay_batches += 1;
        metrics.projections.replay_events_applied += applied;
        metrics.projections.replay_duplicates_skipped += skipped;
        metrics.projections.last_projection = projection.into();
    }

    pub fn record_projection_replay_error(
        &self,
        projection: impl Into<String>,
        error: impl Into<String>,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.replay_errors += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_error = error.into();
    }

    pub fn record_materialized_projection_build(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.materialized_builds += 1;
        metrics.projections.last_projection = projection.into();
    }

    pub fn record_materialized_projection_refresh(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.materialized_refreshes += 1;
        metrics.projections.last_projection = projection.into();
    }

    pub fn record_projection_swap(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.version_swaps += 1;
        metrics.projections.last_projection = projection.into();
    }

    pub fn record_projection_stale_mark(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.stale_marks += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "stale".to_string();
    }

    pub fn record_projection_hash_update(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.row_hash_updates += 1;
        metrics.projections.range_hash_updates += 1;
        metrics.projections.root_hash_updates += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "current".to_string();
    }

    pub fn record_projection_rebuild_verification(
        &self,
        projection: impl Into<String>,
        failed: bool,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.rebuild_verifications += 1;
        if failed {
            metrics.projections.rebuild_verification_failures += 1;
            metrics.projections.last_state = "failed".to_string();
        } else {
            metrics.projections.last_state = "verified".to_string();
        }
        metrics.projections.last_projection = projection.into();
    }

    pub fn record_projection_integrity_verification(
        &self,
        projection: impl Into<String>,
        failed: bool,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.integrity_verifications += 1;
        if failed {
            metrics.projections.integrity_verification_failures += 1;
            metrics.projections.last_state = "failed".to_string();
        } else {
            metrics.projections.last_state = "verified".to_string();
        }
        metrics.projections.last_projection = projection.into();
    }

    pub fn record_mixed_execution_optimized(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.mixed_execution_optimized += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "optimized".to_string();
    }

    pub fn record_mixed_execution_fallback(
        &self,
        projection: impl Into<String>,
        reason: impl Into<String>,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.mixed_execution_fallbacks += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "fallback".to_string();
        metrics.projections.last_fallback_reason = reason.into();
    }
}
