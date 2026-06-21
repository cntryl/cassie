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
    }
}
