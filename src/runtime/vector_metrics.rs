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

    /// Records the generation an ANN query served from, after its final
    /// source-generation check passed.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub(crate) fn record_ann_observed_generation(&self, generation: u64) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.last_ann_observed_generation = generation;
    }

    /// Records an ANN attempt discarded because its source generation moved.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub(crate) fn record_ann_stale_attempt_discard(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.ann_stale_attempt_discards_total += 1;
    }

    /// Records the duration of an exact query answered for an indexed field.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub(crate) fn record_ann_fallback_duration(&self, elapsed: std::time::Duration) {
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.vector.ann_fallback_micros_total += micros;
        metrics.vector.ann_last_fallback_micros = micros;
    }
}
