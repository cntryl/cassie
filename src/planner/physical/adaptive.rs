use super::*;
use crate::config::CassieRuntimeLimits;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OperatorFeedbackPlanDiagnostics {
    pub state: String,
    pub reason: String,
    pub base_candidate: String,
    pub selected_candidate: String,
    pub base_selected_cost: u64,
    pub adjusted_selected_cost: u64,
    pub confidence_bps: u16,
    pub age_ms: u64,
    pub samples: u64,
    pub outlier_samples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdaptivePlanDiagnostics {
    pub enabled: bool,
    pub decision_point: String,
    pub candidates: Vec<String>,
    pub base_alternative: String,
    pub selected_alternative: String,
    pub fallback_operator: String,
    pub guard: String,
    pub guard_passed: bool,
    pub reason: String,
    pub diagnostic: String,
}

pub(crate) fn select_adaptive_read_operator(
    selection: &ReadOperatorSelection,
    operator_selected_index: Option<String>,
    operator_feedback: &OperatorFeedbackPlanDiagnostics,
    limits: &CassieRuntimeLimits,
) -> (Option<String>, AdaptivePlanDiagnostics) {
    let candidates = selection
        .candidates
        .iter()
        .map(|candidate| candidate.label.clone())
        .collect::<Vec<_>>();
    let Some(base_candidate) = selection
        .candidates
        .iter()
        .find(|candidate| candidate.base_selected)
        .or_else(|| selection.candidates.first())
    else {
        return (
            operator_selected_index,
            AdaptivePlanDiagnostics {
                enabled: limits.adaptive_execution_enabled,
                decision_point: "none".to_string(),
                candidates,
                base_alternative: "none".to_string(),
                selected_alternative: "none".to_string(),
                fallback_operator: "none".to_string(),
                guard: "none".to_string(),
                guard_passed: false,
                reason: "no_candidates".to_string(),
                diagnostic: "access_path".to_string(),
            },
        );
    };

    let operator_candidate = selection
        .candidates
        .iter()
        .find(|candidate| candidate.selected_index == operator_selected_index)
        .unwrap_or(base_candidate);
    let threshold_bps = limits.adaptive_min_cost_savings_bps.min(10_000);
    let guard = format!("operator_feedback_cost_savings_bps>={threshold_bps}");
    let mut diagnostics = AdaptivePlanDiagnostics {
        enabled: limits.adaptive_execution_enabled,
        decision_point: "read_operator".to_string(),
        candidates,
        base_alternative: base_candidate.label.clone(),
        selected_alternative: operator_candidate.label.clone(),
        fallback_operator: base_candidate.label.clone(),
        guard,
        guard_passed: false,
        reason: "disabled".to_string(),
        diagnostic: "access_path".to_string(),
    };

    if !limits.adaptive_execution_enabled {
        return (operator_selected_index, diagnostics);
    }

    if selection.candidates.len() <= 1 {
        diagnostics.selected_alternative = base_candidate.label.clone();
        diagnostics.reason = "no_alternatives".to_string();
        return (base_candidate.selected_index.clone(), diagnostics);
    }

    if operator_feedback.state != "used" || operator_candidate.label == base_candidate.label {
        diagnostics.selected_alternative = base_candidate.label.clone();
        diagnostics.reason = "no_runtime_observation".to_string();
        return (base_candidate.selected_index.clone(), diagnostics);
    }

    let savings_bps = cost_savings_bps(
        operator_feedback.base_selected_cost,
        operator_feedback.adjusted_selected_cost,
    );
    diagnostics.guard =
        format!("operator_feedback_cost_savings_bps:{savings_bps}>={threshold_bps}");
    diagnostics.guard_passed = savings_bps >= threshold_bps;
    if diagnostics.guard_passed {
        diagnostics.reason = "selected_operator_feedback".to_string();
        diagnostics.selected_alternative = operator_candidate.label.clone();
        (operator_candidate.selected_index.clone(), diagnostics)
    } else {
        diagnostics.reason = "guard_failed".to_string();
        diagnostics.selected_alternative = base_candidate.label.clone();
        (base_candidate.selected_index.clone(), diagnostics)
    }
}

fn cost_savings_bps(base_cost: u64, selected_cost: u64) -> usize {
    if base_cost == 0 || selected_cost >= base_cost {
        return 0;
    }

    base_cost
        .saturating_sub(selected_cost)
        .saturating_mul(10_000)
        .checked_div(base_cost)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default()
}
