use super::RuntimeState;

#[derive(Debug, Default)]
pub(crate) struct HybridRetrievalDiagnostics {
    pub(crate) posting_reads: usize,
    pub(crate) ann_reads: usize,
    pub(crate) candidate_row_fetches: usize,
    pub(crate) generation_rejections: usize,
    pub(crate) exact_reranks: usize,
    pub(crate) truncations: usize,
    pub(crate) budget_rejections: usize,
}

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub(crate) fn record_hybrid_retrieval_diagnostics(
        &self,
        diagnostics: &HybridRetrievalDiagnostics,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.hybrid.retrieval_stage_queries_total += 1;
        metrics.hybrid.posting_reads_total += diagnostics.posting_reads as u64;
        metrics.hybrid.ann_reads_total += diagnostics.ann_reads as u64;
        metrics.hybrid.candidate_row_fetches_total += diagnostics.candidate_row_fetches as u64;
        metrics.hybrid.generation_rejections_total += diagnostics.generation_rejections as u64;
        metrics.hybrid.exact_reranks_total += diagnostics.exact_reranks as u64;
        metrics.hybrid.truncation_count_total += diagnostics.truncations as u64;
        metrics.hybrid.candidate_budget_rejections_total += diagnostics.budget_rejections as u64;
    }
}
