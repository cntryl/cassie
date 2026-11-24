use std::cell::{Cell, RefCell};

use cntryl_stress::{black_box, LogicalUnit, OperationOutcome};

use super::{
    declared_result_cardinality, prepare_batch_case, record_observed_evidence,
    BenchmarkObservation, BenchmarkTier, BenchmarkTimingMode, CassieStressRunner,
    RuntimeEvidenceObservation, StressCase,
};

impl CassieStressRunner {
    /// Measures a fixed-duration Tier 3-6 batch.
    ///
    /// # Panics
    ///
    /// Panics when called by Tier 1 or 2, or when the case violates the registry contract.
    pub fn measure_batch<F, R>(&mut self, case: StressCase, logical_operations: u64, f: F)
    where
        F: FnMut() -> R,
        R: BenchmarkObservation,
    {
        self.require_duration_batch_tier("measure_batch");
        let case = self.prepare_case(case, BenchmarkTimingMode::Batch);
        self.run_batch(case, logical_operations, f);
    }

    /// Measures a fixed-duration Tier 3-6 batch with fresh untimed setup for every invocation.
    ///
    /// # Panics
    ///
    /// Panics when called by Tier 1 or 2, or when the case violates the registry contract.
    pub fn measure_batch_with_setup<S, F, I, R>(
        &mut self,
        case: StressCase,
        logical_operations: u64,
        setup: S,
        f: F,
    ) where
        S: FnMut() -> I,
        F: FnMut(I) -> R,
        R: BenchmarkObservation,
    {
        self.require_duration_batch_tier("measure_batch_with_setup");
        let case = self.prepare_case(case, BenchmarkTimingMode::Batch);
        self.run_batch_with_setup(case, logical_operations, setup, f);
    }

    fn require_duration_batch_tier(&self, operation: &str) {
        assert!(
            matches!(
                self.tier,
                BenchmarkTier::Tier3
                    | BenchmarkTier::Tier4
                    | BenchmarkTier::Tier5
                    | BenchmarkTier::Tier6
            ),
            "{operation} is only valid for Tiers 3-6"
        );
    }

    fn run_batch<F, R>(&mut self, case: StressCase, logical_operations: u64, f: F)
    where
        F: FnMut() -> R,
        R: BenchmarkObservation,
    {
        let f = RefCell::new(f);
        let declared_cardinality = declared_result_cardinality(&case);
        let evidence = case.runtime_evidence.clone();
        let preflight = case.preflight_evidence.clone();
        let scenario = self.scenario_for(&case);
        let case = prepare_batch_case(case, logical_operations);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let observation = Cell::new(BatchObservation::default());
            let completed = ctx.measure_batch(&measurement_name, logical_operations, || {
                let result = (f.borrow_mut())();
                observation.set(BatchObservation::from_result(&result));
                black_box(result);
            });
            record_batch_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                declared_cardinality,
                observation.get(),
                completed,
            );
        });
    }

    fn run_batch_with_setup<S, F, I, R>(
        &mut self,
        case: StressCase,
        logical_operations: u64,
        setup: S,
        f: F,
    ) where
        S: FnMut() -> I,
        F: FnMut(I) -> R,
        R: BenchmarkObservation,
    {
        let setup = RefCell::new(setup);
        let f = RefCell::new(f);
        let logical_unit = case
            .runtime_declaration
            .as_ref()
            .expect("validated runtime declaration")
            .operation_unit()
            .as_str();
        let declared_cardinality = declared_result_cardinality(&case);
        let evidence = case.runtime_evidence.clone();
        let preflight = case.preflight_evidence.clone();
        let scenario = self.scenario_for(&case);
        let case = prepare_batch_case(case, logical_operations);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let observation = Cell::new(BatchObservation::default());
            let completed = ctx
                .measure_outcome_with_setup(
                    &measurement_name,
                    LogicalUnit::new(logical_unit),
                    || (setup.borrow_mut())(),
                    |input| {
                        let result = (f.borrow_mut())(input);
                        observation.set(BatchObservation::from_result(&result));
                        black_box(result);
                        OperationOutcome::success(logical_operations)
                    },
                )
                .completed;
            record_batch_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                declared_cardinality,
                observation.get(),
                completed,
            );
        });
    }
}

#[derive(Clone, Copy, Default)]
struct BatchObservation {
    cardinality: u64,
    candidate_count: Option<u64>,
    peak_query_memory_bytes: Option<u64>,
}

impl BatchObservation {
    fn from_result(result: &impl BenchmarkObservation) -> Self {
        Self {
            cardinality: result.cardinality(),
            candidate_count: result.candidate_count(),
            peak_query_memory_bytes: result.peak_query_memory_bytes(),
        }
    }
}

fn record_batch_evidence(
    context: &mut cntryl_stress::StressContext,
    source: Option<&super::RuntimeEvidenceSource>,
    scenario: &super::performance_benchmarks::PerformanceBenchmarkScenario,
    preflight: Option<&super::PreflightEvidence>,
    declared_cardinality: Option<u64>,
    observation: BatchObservation,
    completed: u64,
) {
    context.metadata("failed_operations", 0);
    record_observed_evidence(
        context,
        source,
        scenario,
        preflight,
        RuntimeEvidenceObservation::new(
            declared_cardinality.unwrap_or(observation.cardinality),
            observation.candidate_count,
            observation.peak_query_memory_bytes,
        )
        .per_external_operation(completed),
    );
}
