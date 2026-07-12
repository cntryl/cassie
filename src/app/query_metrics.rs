use super::query_explain::{
    QueryPlanAnalyze, QueryPlanAnalyzeDiagnostics, QueryPlanOperatorActual,
};
use crate::app::{CassieError, QueryResult, RuntimeFeedbackObservation};
use std::fmt::Write as _;

pub(crate) struct RuntimeFeedbackDeltas {
    pub(crate) storage_reads: u64,
    pub(crate) storage_writes: u64,
    pub(crate) temp_writes: u64,
    pub(crate) candidate_count: u64,
    pub(crate) result_count: u64,
}

impl RuntimeFeedbackDeltas {
    pub(crate) fn from_snapshots(
        before: &crate::runtime::RuntimeMetricsSnapshot,
        after: &crate::runtime::RuntimeMetricsSnapshot,
    ) -> Self {
        Self {
            storage_reads: after
                .storage
                .data
                .reads
                .saturating_sub(before.storage.data.reads),
            storage_writes: after
                .storage
                .data
                .writes
                .saturating_sub(before.storage.data.writes),
            temp_writes: after
                .storage
                .temp
                .writes
                .saturating_sub(before.storage.temp.writes),
            candidate_count: search_candidate_delta(before, after),
            result_count: search_result_delta(before, after),
        }
    }

    pub(crate) fn to_observation(
        &self,
        execution: &Result<QueryResult, CassieError>,
        elapsed_ms: u64,
    ) -> RuntimeFeedbackObservation {
        RuntimeFeedbackObservation {
            rows_in: self.storage_reads.saturating_add(self.candidate_count).max(
                execution
                    .as_ref()
                    .map_or(0, |result| result.rows.len() as u64),
            ),
            rows_out: execution
                .as_ref()
                .map_or(0, |result| result.rows.len() as u64),
            elapsed_ms,
            storage_reads: self.storage_reads,
            storage_writes: self.storage_writes,
            temp_writes: self.temp_writes,
            candidate_count: self.candidate_count,
            result_count: self.result_count,
            error_class: execution
                .as_ref()
                .err()
                .map(|error| crate::runtime::error_class(error).to_string()),
            spilled: self.temp_writes > 0,
            memory_pressure: self.temp_writes > 0,
        }
    }
}

pub(crate) struct ExplainAnalyzeReport {
    pub(crate) result: QueryResult,
    pub(crate) elapsed_ms: u128,
    pub(crate) deltas: ExplainAnalyzeDeltas,
}

pub(crate) struct ExplainAnalyzeDeltas {
    pub(crate) runtime: RuntimeFeedbackDeltas,
    pub(crate) plan_cache_hits: u64,
    pub(crate) plan_cache_misses: u64,
    pub(crate) parallel_aggregations: u64,
    pub(crate) parallel_aggregation_fallbacks: u64,
    pub(crate) parallel_aggregation_workers: u64,
    pub(crate) parallel_aggregation_groups: u64,
    pub(crate) adaptive_plan_decisions: u64,
    pub(crate) adaptive_plan_selected: u64,
    pub(crate) operator_switch_attempts: u64,
    pub(crate) operator_switch_successes: u64,
    pub(crate) operator_switch_skips: u64,
    pub(crate) operator_switch_fallbacks: u64,
}

impl ExplainAnalyzeDeltas {
    pub(crate) fn from_snapshots(
        before: &crate::runtime::RuntimeMetricsSnapshot,
        after: &crate::runtime::RuntimeMetricsSnapshot,
    ) -> Self {
        Self {
            runtime: RuntimeFeedbackDeltas::from_snapshots(before, after),
            plan_cache_hits: after.plan_cache.hits.saturating_sub(before.plan_cache.hits),
            plan_cache_misses: after
                .plan_cache
                .misses
                .saturating_sub(before.plan_cache.misses),
            parallel_aggregations: after
                .parallel_aggregation
                .aggregations
                .saturating_sub(before.parallel_aggregation.aggregations),
            parallel_aggregation_fallbacks: after
                .parallel_aggregation
                .fallback_aggregations
                .saturating_sub(before.parallel_aggregation.fallback_aggregations),
            parallel_aggregation_workers: after
                .parallel_aggregation
                .workers
                .saturating_sub(before.parallel_aggregation.workers),
            parallel_aggregation_groups: after
                .parallel_aggregation
                .groups
                .saturating_sub(before.parallel_aggregation.groups),
            adaptive_plan_decisions: after
                .adaptive_candidates
                .plan_decisions
                .saturating_sub(before.adaptive_candidates.plan_decisions),
            adaptive_plan_selected: after
                .adaptive_candidates
                .plan_selected_alternatives
                .saturating_sub(before.adaptive_candidates.plan_selected_alternatives),
            operator_switch_attempts: after
                .adaptive_candidates
                .operator_switch_attempts
                .saturating_sub(before.adaptive_candidates.operator_switch_attempts),
            operator_switch_successes: after
                .adaptive_candidates
                .operator_switch_successes
                .saturating_sub(before.adaptive_candidates.operator_switch_successes),
            operator_switch_skips: after
                .adaptive_candidates
                .operator_switch_skips
                .saturating_sub(before.adaptive_candidates.operator_switch_skips),
            operator_switch_fallbacks: after
                .adaptive_candidates
                .operator_switch_fallbacks
                .saturating_sub(before.adaptive_candidates.operator_switch_fallbacks),
        }
    }

    pub(crate) fn to_success_observation(
        &self,
        result: &QueryResult,
        elapsed_ms: u64,
    ) -> RuntimeFeedbackObservation {
        RuntimeFeedbackObservation {
            rows_in: self
                .runtime
                .storage_reads
                .saturating_add(self.runtime.candidate_count)
                .max(result.rows.len() as u64),
            rows_out: result.rows.len() as u64,
            elapsed_ms,
            storage_reads: self.runtime.storage_reads,
            storage_writes: self.runtime.storage_writes,
            temp_writes: self.runtime.temp_writes,
            candidate_count: self.runtime.candidate_count,
            result_count: self.runtime.result_count,
            error_class: None,
            spilled: self.runtime.temp_writes > 0,
            memory_pressure: self.runtime.temp_writes > 0,
        }
    }
}

fn search_candidate_delta(
    before: &crate::runtime::RuntimeMetricsSnapshot,
    after: &crate::runtime::RuntimeMetricsSnapshot,
) -> u64 {
    after
        .search
        .candidate_count_total
        .saturating_sub(before.search.candidate_count_total)
        .saturating_add(
            after
                .vector
                .candidate_count_total
                .saturating_sub(before.vector.candidate_count_total),
        )
        .saturating_add(
            after
                .hybrid
                .candidate_count_total
                .saturating_sub(before.hybrid.candidate_count_total),
        )
}

fn search_result_delta(
    before: &crate::runtime::RuntimeMetricsSnapshot,
    after: &crate::runtime::RuntimeMetricsSnapshot,
) -> u64 {
    after
        .search
        .result_count_total
        .saturating_sub(before.search.result_count_total)
        .saturating_add(
            after
                .vector
                .result_count_total
                .saturating_sub(before.vector.result_count_total),
        )
        .saturating_add(
            after
                .hybrid
                .result_count_total
                .saturating_sub(before.hybrid.result_count_total),
        )
}

pub(crate) fn append_explain_analyze(
    plan: &mut String,
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) {
    let actual_operators = actual_operator_diagnostics(physical, report);
    let deltas = &report.deltas;
    let runtime = &deltas.runtime;
    let _ = write!(
        plan,
        " analyze=true actual_rows={} actual_ms={} operator_actuals={} diagnostics=plan_cache_hits_delta:{},plan_cache_misses_delta:{},storage_reads_delta:{},storage_writes_delta:{},temp_writes_delta:{},candidate_count_delta:{},result_count_delta:{},parallel_aggregations_delta:{},parallel_aggregation_fallback_delta:{},parallel_aggregation_workers_delta:{},parallel_aggregation_groups_delta:{},adaptive_plan_decisions_delta:{},adaptive_plan_selected_delta:{},operator_switch_attempts_delta:{},operator_switch_success_delta:{},operator_switch_skips_delta:{},operator_switch_fallbacks_delta:{}",
        report.result.rows.len(),
        report.elapsed_ms,
        actual_operators,
        deltas.plan_cache_hits,
        deltas.plan_cache_misses,
        runtime.storage_reads,
        runtime.storage_writes,
        runtime.temp_writes,
        runtime.candidate_count,
        runtime.result_count,
        deltas.parallel_aggregations,
        deltas.parallel_aggregation_fallbacks,
        deltas.parallel_aggregation_workers,
        deltas.parallel_aggregation_groups,
        deltas.adaptive_plan_decisions,
        deltas.adaptive_plan_selected,
        deltas.operator_switch_attempts,
        deltas.operator_switch_successes,
        deltas.operator_switch_skips,
        deltas.operator_switch_fallbacks
    );
}

fn actual_operator_diagnostics(
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> String {
    if physical.operators.is_empty() {
        return "Command".to_string();
    }
    physical
        .operators
        .iter()
        .map(|operator| {
            format!(
                "{operator:?}:rows_in:{} rows_out:{} elapsed_ms:{} storage_reads:{} storage_writes:{} temp_writes:{} candidates:{} results:{}",
                physical.estimates.scan_rows,
                report.result.rows.len(),
                report.elapsed_ms,
                report.deltas.runtime.storage_reads,
                report.deltas.runtime.storage_writes,
                report.deltas.runtime.temp_writes,
                report.deltas.runtime.candidate_count,
                report.deltas.runtime.result_count
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn structured_analyze(
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> QueryPlanAnalyze {
    QueryPlanAnalyze {
        actual_rows: report.result.rows.len(),
        actual_ms: report.elapsed_ms,
        operator_actuals: structured_operator_actuals(physical, report),
        diagnostics: structured_analyze_diagnostics(report),
    }
}

fn structured_operator_actuals(
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> Vec<QueryPlanOperatorActual> {
    if physical.operators.is_empty() {
        return vec![operator_actual("Command", physical, report)];
    }

    physical
        .operators
        .iter()
        .map(|operator| operator_actual(format!("{operator:?}"), physical, report))
        .collect()
}

fn operator_actual(
    operator: impl Into<String>,
    physical: &crate::planner::physical::PhysicalPlan,
    report: &ExplainAnalyzeReport,
) -> QueryPlanOperatorActual {
    QueryPlanOperatorActual {
        operator: operator.into(),
        rows_in: physical.estimates.scan_rows,
        rows_out: report.result.rows.len(),
        elapsed_ms: report.elapsed_ms,
        storage_reads: report.deltas.runtime.storage_reads,
        storage_writes: report.deltas.runtime.storage_writes,
        temp_writes: report.deltas.runtime.temp_writes,
        candidates: report.deltas.runtime.candidate_count,
        results: report.deltas.runtime.result_count,
    }
}

fn structured_analyze_diagnostics(report: &ExplainAnalyzeReport) -> QueryPlanAnalyzeDiagnostics {
    QueryPlanAnalyzeDiagnostics {
        plan_cache_hits: report.deltas.plan_cache_hits,
        plan_cache_misses: report.deltas.plan_cache_misses,
        storage_reads: report.deltas.runtime.storage_reads,
        storage_writes: report.deltas.runtime.storage_writes,
        temp_writes: report.deltas.runtime.temp_writes,
        candidate_count: report.deltas.runtime.candidate_count,
        result_count: report.deltas.runtime.result_count,
        parallel_aggregations: report.deltas.parallel_aggregations,
        parallel_aggregation_fallback: report.deltas.parallel_aggregation_fallbacks,
        parallel_aggregation_workers: report.deltas.parallel_aggregation_workers,
        parallel_aggregation_groups: report.deltas.parallel_aggregation_groups,
        adaptive_plan_decisions: report.deltas.adaptive_plan_decisions,
        adaptive_plan_selected: report.deltas.adaptive_plan_selected,
        operator_switch_attempts: report.deltas.operator_switch_attempts,
        operator_switch_success: report.deltas.operator_switch_successes,
        operator_switch_skips: report.deltas.operator_switch_skips,
        operator_switch_fallbacks: report.deltas.operator_switch_fallbacks,
    }
}
