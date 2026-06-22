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

    pub fn record_adaptive_plan_decision(
        &self,
        diagnostics: &crate::planner::physical::AdaptivePlanDiagnostics,
    ) {
        if diagnostics.decision_point.is_empty() || diagnostics.decision_point == "none" {
            return;
        }

        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.adaptive_candidates.plan_decisions += 1;
        metrics
            .adaptive_candidates
            .plan_candidate_alternatives_total += diagnostics.candidates.len() as u64;
        if !diagnostics.enabled {
            metrics.adaptive_candidates.plan_disabled_total += 1;
        }
        if diagnostics.guard_passed {
            metrics.adaptive_candidates.plan_guard_passed_total += 1;
        } else {
            metrics.adaptive_candidates.plan_guard_failed_total += 1;
        }
        if diagnostics.selected_alternative != diagnostics.base_alternative {
            metrics.adaptive_candidates.plan_selected_alternatives += 1;
        }
        metrics.adaptive_candidates.last_plan_decision_point = diagnostics.decision_point.clone();
        metrics.adaptive_candidates.last_plan_base_alternative =
            diagnostics.base_alternative.clone();
        metrics.adaptive_candidates.last_plan_selected_alternative =
            diagnostics.selected_alternative.clone();
        metrics.adaptive_candidates.last_plan_guard = diagnostics.guard.clone();
        metrics.adaptive_candidates.last_plan_reason = diagnostics.reason.clone();
    }
}
