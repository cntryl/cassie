use super::RuntimeState;

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_ivfflat_execution(&self, lists: usize, probes: usize, exact_reranks: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.ivfflat_executions += 1;
        metrics.vector.ivfflat_lists_total += lists as u64;
        metrics.vector.ivfflat_probes_total += probes as u64;
        metrics.vector.ivfflat_exact_reranks_total += exact_reranks as u64;
        metrics.vector.last_index_kind = "ivfflat".to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_ivfflat_fallback(&self, reason: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.ivfflat_fallbacks += 1;
        metrics.vector.last_index_kind = "ivfflat".to_string();
        metrics.vector.last_fallback_reason = reason.into();
    }
}
