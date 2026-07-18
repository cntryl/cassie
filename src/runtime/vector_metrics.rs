use super::RuntimeState;

impl RuntimeState {
    /// Records persisted ANN reads only after an ANN path is selected successfully.
    pub(crate) fn record_vector_retrieval_diagnostics(
        &self,
        ann_reads: usize,
        candidate_row_fetches: usize,
        exact_reranks: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.retrieval_stage_queries_total += 1;
        metrics.vector.ann_reads_total += ann_reads as u64;
        metrics.vector.candidate_row_fetches_total += candidate_row_fetches as u64;
        metrics.vector.exact_reranks_total += exact_reranks as u64;
    }

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

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_hnsw_execution(&self, exact_reranks: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.hnsw_executions += 1;
        metrics.vector.hnsw_exact_reranks_total += exact_reranks as u64;
        metrics.vector.last_index_kind = "hnsw".to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_hnsw_fallback(&self, reason: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.hnsw_fallbacks += 1;
        metrics.vector.last_index_kind = "hnsw".to_string();
        metrics.vector.last_fallback_reason = reason.into();
    }
}
