//! Full-text retrieval-stage counters of the `search` metrics section.

use super::{increment_boundary_counter, RuntimeState};

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_fulltext_row_scan_fallback(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.search.retrieval_stage_queries_total += 1;
        metrics.search.row_scan_fallback_total += 1;
        increment_boundary_counter(&mut metrics.search.retrieval_fallback_reasons, reason);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_fulltext_retrieval_diagnostics(
        &self,
        posting_reads: usize,
        candidate_row_fetches: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.search.retrieval_stage_queries_total += 1;
        metrics.search.posting_reads_total += posting_reads as u64;
        metrics.search.candidate_row_fetches_total += candidate_row_fetches as u64;
    }

    /// Records one scalar-index intersection of an indexed full-text read:
    /// the posting candidates it started from, the ids the scalar equality
    /// indexes supplied, and the candidates that stayed eligible and matched.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_fulltext_scalar_prefilter(
        &self,
        posting_candidates: usize,
        scalar_candidates: usize,
        eligible_candidates: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.search.prefilter_input_candidate_count_total += posting_candidates as u64;
        metrics.search.scalar_candidate_count_total += scalar_candidates as u64;
        metrics.search.prefilter_filtered_candidate_count_total += eligible_candidates as u64;
    }

    /// Names why an indexed full-text path handed a query to the exact row
    /// scan, as `<path>:<reason>` in the fallback reasons. The row scan itself
    /// counts the fallback, so this adds only the cause.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_fulltext_fallback_cause(&self, path: &str, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(
            &mut metrics.search.retrieval_fallback_reasons,
            &format!("{path}:{reason}"),
        );
    }
}
