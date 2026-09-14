use std::cell::{Cell, RefCell};

use super::{
    declared_result_cardinality, performance_benchmarks, record_observed_evidence, stress_evidence,
    BenchmarkSpec, BenchmarkTier, BenchmarkTimingMode, CassieStressRunner, Duration, Instant,
    MeasurementIntent, PreflightEvidence, RuntimeEvidenceObservation, StressCase,
};

struct InterleavedCountedCase {
    input_index: usize,
    measurement_name: String,
    spec: BenchmarkSpec,
    scenario: &'static performance_benchmarks::PerformanceBenchmarkScenario,
    declared_cardinality: Option<u64>,
    preflight: Option<PreflightEvidence>,
}

pub(crate) fn alternating_pair_execution_order(
    case_count: usize,
    invocation_index: usize,
) -> Vec<usize> {
    let mut order = Vec::with_capacity(case_count.saturating_mul(2));
    for first in (0..case_count).step_by(2) {
        let Some(second) = first.checked_add(1).filter(|second| *second < case_count) else {
            order.extend([first, first]);
            continue;
        };
        if invocation_index.is_multiple_of(2) {
            order.extend([first, second, second, first]);
        } else {
            order.extend([second, first, first, second]);
        }
    }
    order
}

pub(crate) fn alternating_pair_chunk_execution_order(
    case_count: usize,
    iterations_per_case: usize,
    invocation_index: usize,
) -> Vec<usize> {
    assert!(
        iterations_per_case > 0 && iterations_per_case.is_multiple_of(2),
        "balanced pair chunks require a positive even iteration count"
    );
    let mut order = Vec::with_capacity(case_count.saturating_mul(iterations_per_case));
    for block_index in 0..(iterations_per_case / 2) {
        order.extend(alternating_pair_execution_order(
            case_count,
            invocation_index.wrapping_add(block_index),
        ));
    }
    order
}

pub(crate) fn adjacent_pair_execution_groups(case_count: usize) -> Vec<std::ops::Range<usize>> {
    (0..case_count)
        .step_by(2)
        .map(|first| first..first.saturating_add(2).min(case_count))
        .collect()
}

fn assert_matching_shapes(cases: &[InterleavedCountedCase]) {
    let first_spec = &cases[0].spec;
    for case in &cases[1..] {
        assert!(
            first_spec.parameters.keys().eq(case.spec.parameters.keys()),
            "interleaved counted measurements require identical parameter keys"
        );
        assert!(
            first_spec.metadata.keys().eq(case.spec.metadata.keys()),
            "interleaved counted measurements require identical metadata keys"
        );
    }
}

impl CassieStressRunner {
    fn prepare_interleaved_counted_cases(
        &mut self,
        cases: Vec<StressCase>,
    ) -> (usize, Vec<InterleavedCountedCase>) {
        let case_count = cases.len();
        let mut selected_cases = Vec::new();
        for (input_index, case) in cases.into_iter().enumerate() {
            let case = self.prepare_case(case, BenchmarkTimingMode::Counted);
            self.validate_registry_case(&case);
            self.validate_runtime_case(&case);
            if !self.should_run(&case) {
                continue;
            }
            assert!(
                case.runtime_evidence.is_none(),
                "interleaved counted measurements do not support runtime counter evidence"
            );
            let fixture = case
                .runtime_declaration
                .as_ref()
                .expect("validated runtime declaration")
                .fixture();
            self.fixture_identities
                .register(self.suite, &case.fixture_scale, fixture.identity())
                .unwrap_or_else(|error| panic!("invalid benchmark fixture reuse: {error}"));
            let scenario = self.scenario_for(&case);
            stress_evidence::validate_preflight_requirement(
                scenario,
                case.preflight_evidence.as_ref(),
            )
            .unwrap_or_else(|error| panic!("invalid benchmark evidence: {error}"));
            let declared_cardinality = declared_result_cardinality(&case);
            let preflight = case.preflight_evidence.clone();
            let measurement_name = case.measurement_name();
            let case = case
                .intent(MeasurementIntent::External)
                .metadata("logical_operations_source", "completed_count");
            selected_cases.push(InterleavedCountedCase {
                input_index,
                measurement_name,
                spec: self.spec_for(case),
                scenario,
                declared_cardinality,
                preflight,
            });
        }
        (case_count, selected_cases)
    }

    /// Measures adjacent Tier 2 candidate/baseline rows in independent runner groups.
    ///
    /// Each row runs in one-operation chunks. Their execution order alternates balanced `ABBA` and
    /// `BAAB` blocks within and across invocations, while measurements are recorded in stable
    /// declaration order as required by `cntryl-stress`. This prevents host drift or one transient
    /// delay from systematically favoring the row measured first.
    ///
    /// # Panics
    ///
    /// Panics when called by another tier, when a selected case violates the registry contract,
    /// when selected cases do not expose identical parameter and metadata keys, when runtime
    /// counter evidence is attached, or when no operations complete.
    pub fn measure_counted_interleaved<F>(
        &mut self,
        cases: Vec<StressCase>,
        iterations_per_case: usize,
        f: F,
    ) where
        F: FnMut(usize, usize) -> u64,
    {
        self.require_tier(BenchmarkTier::Tier2, "measure_counted_interleaved");
        assert!(
            iterations_per_case > 0 && iterations_per_case.is_multiple_of(2),
            "interleaved counted measurements require a positive even iteration count"
        );
        let (case_count, selected_cases) = self.prepare_interleaved_counted_cases(cases);
        if selected_cases.is_empty() {
            return;
        }

        assert_matching_shapes(&selected_cases);
        self.selected = self.selected.saturating_add(selected_cases.len());
        let f = RefCell::new(f);
        for group in adjacent_pair_execution_groups(case_count) {
            let grouped_cases = selected_cases
                .iter()
                .filter(|case| group.contains(&case.input_index))
                .collect::<Vec<_>>();
            let Some(first_case) = grouped_cases.first() else {
                continue;
            };
            let mut group_spec = first_case.spec.clone();
            group_spec.id = format!(
                "{}/interleaved-counted/{}",
                self.suite, first_case.scenario.scenario_id
            );
            group_spec.name = format!("interleaved-counted-{}", first_case.scenario.workload);
            group_spec.parameters.clear();
            group_spec.metadata.clear();

            let invocation = Cell::new(0usize);
            self.runner.run_spec(&group_spec, |context| {
                let invocation_index = invocation.get();
                invocation.set(invocation_index.saturating_add(1));
                let mut observations = vec![(Duration::ZERO, 0_u64); grouped_cases.len()];
                for case_index in alternating_pair_chunk_execution_order(
                    grouped_cases.len(),
                    iterations_per_case,
                    invocation_index,
                ) {
                    let started = Instant::now();
                    let completed = (f.borrow_mut())(grouped_cases[case_index].input_index, 1);
                    observations[case_index].0 += started.elapsed();
                    observations[case_index].1 =
                        observations[case_index].1.saturating_add(completed);
                }

                for (case_index, case) in grouped_cases.iter().enumerate() {
                    let (elapsed, completed) = observations[case_index];
                    context.record_external(&case.measurement_name, elapsed, completed);
                    for (key, value) in &case.spec.parameters {
                        context.parameter(key, value);
                    }
                    for (key, value) in &case.spec.metadata {
                        context.metadata(key, value);
                    }
                    context.metadata("failed_operations", 0);
                    record_observed_evidence(
                        context,
                        None,
                        case.scenario,
                        case.preflight.as_ref(),
                        RuntimeEvidenceObservation::new(
                            case.declared_cardinality.unwrap_or(completed),
                            None,
                            None,
                        ),
                    );
                }
            });
        }
    }
}
