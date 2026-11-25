use super::{RollupGroupSnapshot, RuntimeState};

impl RuntimeState {
    pub(crate) fn record_incremental_rollup_groups(&self, groups: Vec<RollupGroupSnapshot>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        let recomputed = u64::try_from(groups.len()).unwrap_or(u64::MAX);
        metrics.rollups.incremental_groups_recomputed = metrics
            .rollups
            .incremental_groups_recomputed
            .saturating_add(recomputed);
        metrics.rollups.last_incremental_groups = groups;
    }

    pub(crate) fn record_incremental_rollup_fallback(&self, reason: impl Into<String>) {
        self.metrics
            .lock()
            .expect("runtime metrics")
            .rollups
            .last_incremental_fallback_reason = reason.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rollup_refresh(&self, name: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.rollups.refreshes += 1;
        metrics.rollups.last_rollup = name.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rollup_rewrite(&self, name: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.rollups.rewrite_hits += 1;
        metrics.rollups.last_rollup = name.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_rollup_fallback(&self, reason: impl Into<String>) {
        let reason = reason.into();
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.rollups.fallback_scans += 1;
        if reason == "stale" {
            metrics.rollups.stale_fallbacks += 1;
        }
        metrics.rollups.last_fallback_reason = reason;
    }
}
