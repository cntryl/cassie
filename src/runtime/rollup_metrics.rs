use super::RuntimeState;

impl RuntimeState {
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
