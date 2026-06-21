use super::*;

impl RuntimeState {
    pub fn record_adaptive_candidate_decision(
        &self,
        initial_budget: usize,
        feedback_budget: Option<usize>,
        expansions: usize,
        final_candidate_count: usize,
        exhausted: bool,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.adaptive_candidates.decisions += 1;
        metrics.adaptive_candidates.initial_budget_total += initial_budget as u64;
        metrics.adaptive_candidates.feedback_budget_total +=
            feedback_budget.unwrap_or_default() as u64;
        metrics.adaptive_candidates.expansions_total += expansions as u64;
        metrics.adaptive_candidates.final_candidate_count_total += final_candidate_count as u64;
        if exhausted {
            metrics.adaptive_candidates.exhausted_total += 1;
        }
    }

    pub fn record_adaptive_candidate_limit_error(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.adaptive_candidates.limit_errors_total += 1;
    }
}
