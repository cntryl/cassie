use cntryl_stress::artifact::{BenchmarkSummary, StressRun};

#[derive(Debug, Clone)]
pub(super) struct RelativeP95Gate {
    candidate_scenario: &'static str,
    baseline_scenario: &'static str,
    maximum_ratio: f64,
}

impl RelativeP95Gate {
    pub(super) const fn new(
        candidate_scenario: &'static str,
        baseline_scenario: &'static str,
        maximum_ratio: f64,
    ) -> Self {
        Self {
            candidate_scenario,
            baseline_scenario,
            maximum_ratio,
        }
    }
}

pub(super) fn validate_relative_p95_gates(
    run: &StressRun,
    gates: &[RelativeP95Gate],
) -> Result<(), String> {
    for gate in gates {
        let candidate = summary_for_scenario(run, gate.candidate_scenario)?;
        let baseline = summary_for_scenario(run, gate.baseline_scenario)?;
        let candidate_p95 = p95_nanoseconds(candidate)?;
        let baseline_p95 = p95_nanoseconds(baseline)?;
        validate_ratio(
            gate.candidate_scenario,
            candidate_p95,
            gate.baseline_scenario,
            baseline_p95,
            gate.maximum_ratio,
        )?;
    }
    Ok(())
}

fn summary_for_scenario<'a>(
    run: &'a StressRun,
    scenario: &str,
) -> Result<&'a BenchmarkSummary, String> {
    run.summaries
        .iter()
        .find(|summary| {
            summary
                .metadata
                .get("scenario_id")
                .is_some_and(|candidate| candidate == scenario)
        })
        .ok_or_else(|| format!("missing relative-p95 scenario '{scenario}'"))
}

fn p95_nanoseconds(summary: &BenchmarkSummary) -> Result<f64, String> {
    summary
        .ns_per_op
        .as_ref()
        .or(summary.stats.as_ref())
        .map(|stats| stats.p95)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| {
            format!(
                "scenario '{}' has no finite p95 latency",
                summary
                    .metadata
                    .get("scenario_id")
                    .map_or(summary.benchmark_id.as_str(), String::as_str)
            )
        })
}

fn validate_ratio(
    candidate: &str,
    candidate_p95: f64,
    baseline: &str,
    baseline_p95: f64,
    maximum_ratio: f64,
) -> Result<(), String> {
    if !maximum_ratio.is_finite() || maximum_ratio <= 0.0 {
        return Err("relative p95 maximum ratio must be positive and finite".to_string());
    }
    if !baseline_p95.is_finite() || baseline_p95 <= 0.0 {
        return Err(format!(
            "relative p95 baseline '{baseline}' must be positive and finite"
        ));
    }
    let ratio = candidate_p95 / baseline_p95;
    if ratio > maximum_ratio {
        return Err(format!(
            "relative p95 gate failed: '{candidate}' p95 {candidate_p95:.0}ns is {ratio:.3}x '{baseline}' p95 {baseline_p95:.0}ns; maximum {maximum_ratio:.3}x"
        ));
    }
    Ok(())
}
