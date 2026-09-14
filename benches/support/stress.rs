use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cntryl_stress::{
    artifact::{BenchmarkBudgets, BenchmarkModeKind, BenchmarkSpec, MeasurementIntent, RunProfile},
    black_box,
    runner::{evaluate_run_gate, RunGate},
    StressContext, StressRunner, StressRunnerConfig,
};

use crate::performance_benchmarks::{self, BenchmarkTier, BenchmarkTimingMode};

#[path = "stress_evidence.rs"]
mod stress_evidence;
pub use stress_evidence::{
    normalize_runtime_counter, scoped_candidate_count, scoped_fallback_evidence,
    scoped_storage_read_observation, validate_preflight_requirement, PreflightEvidence,
};
use stress_evidence::{RuntimeEvidenceObservation, RuntimeEvidenceSource};
#[path = "stress_config.rs"]
mod stress_config;
use stress_config::{matches_filter, print_config, resolve_config};
#[path = "stress_runtime_contract.rs"]
mod stress_runtime_contract;
pub use stress_runtime_contract::{
    validate_runtime_case_contract, FixtureDeclaration, FixtureIdentityTracker, OperationUnit,
    RuntimeCaseDeclaration,
};
#[path = "stress_relative_gates.rs"]
mod stress_relative_gates;
use stress_relative_gates::{validate_relative_p95_gates, RelativeP95Gate};
#[path = "stress_interleaved.rs"]
pub(crate) mod stress_interleaved;
#[path = "stress_soak.rs"]
mod stress_soak;
pub use stress_soak::{
    resolve_soak_duration, soak_sample_duration, validate_soak_duration_for_profile, SoakDuration,
};

#[must_use]
/// Returns the elapsed time for one external sample.
///
/// # Panics
///
/// Panics when the external harness reports zero completed operations.
pub fn external_elapsed(elapsed: Duration, completed_operations: u64) -> Duration {
    assert!(
        completed_operations > 0,
        "external measurements require completed operations"
    );
    elapsed
}

pub struct CassieStressRunner {
    suite: &'static str,
    tier: BenchmarkTier,
    config: StressRunnerConfig,
    filter: Option<String>,
    tier_filter: Option<u32>,
    baseline: Option<PathBuf>,
    runner: StressRunner,
    selected: usize,
    fixture_identities: FixtureIdentityTracker,
    relative_p95_gates: Vec<RelativeP95Gate>,
}

#[derive(Clone)]
pub struct StressCase {
    tier: u32,
    mode: BenchmarkModeKind,
    workload: String,
    fixture_scale: String,
    parameters: BTreeMap<String, String>,
    metadata: BTreeMap<String, String>,
    intent: MeasurementIntent,
    runtime_evidence: Option<RuntimeEvidenceSource>,
    preflight_evidence: Option<PreflightEvidence>,
    runtime_declaration: Option<RuntimeCaseDeclaration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalSample {
    pub elapsed: Duration,
    pub completed_operations: u64,
}

impl ExternalSample {
    #[must_use]
    pub const fn new(elapsed: Duration, completed_operations: u64) -> Self {
        Self {
            elapsed,
            completed_operations,
        }
    }
}

impl StressCase {
    pub fn new(workload: impl Into<String>, fixture_scale: impl Into<String>) -> Self {
        Self {
            tier: 0,
            mode: BenchmarkModeKind::FixedOperations,
            workload: workload.into(),
            fixture_scale: fixture_scale.into(),
            parameters: BTreeMap::new(),
            metadata: BTreeMap::new(),
            intent: MeasurementIntent::General,
            runtime_evidence: None,
            preflight_evidence: None,
            runtime_declaration: None,
        }
    }

    #[must_use]
    pub fn parameter(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn runtime_evidence(mut self, cassie: Arc<cassie::app::Cassie>) -> Self {
        self.runtime_evidence = Some(RuntimeEvidenceSource::new(cassie));
        self
    }

    #[must_use]
    pub fn runtime_state_evidence(mut self, runtime: Arc<cassie::runtime::RuntimeState>) -> Self {
        self.runtime_evidence = Some(RuntimeEvidenceSource::from_runtime(runtime));
        self
    }

    #[must_use]
    pub fn runtime_contract(
        mut self,
        fixture: FixtureDeclaration,
        operation_unit: OperationUnit,
    ) -> Self {
        self.runtime_declaration = Some(RuntimeCaseDeclaration::new(fixture, operation_unit));
        self
    }

    /// Attaches access-path and fallback evidence observed during untimed preflight.
    #[must_use]
    pub fn preflight_evidence(
        mut self,
        selected_access_path: impl Into<String>,
        fallback_reason: impl Into<String>,
    ) -> Self {
        self.preflight_evidence = Some(PreflightEvidence::new(
            selected_access_path,
            fallback_reason,
        ));
        self
    }

    #[must_use]
    pub fn informational(mut self, reason: impl Into<String>) -> Self {
        self.metadata
            .insert("signal_role".to_string(), "informational".to_string());
        self.metadata
            .insert("signal_reason".to_string(), reason.into());
        self
    }

    fn intent(mut self, intent: MeasurementIntent) -> Self {
        self.intent = intent;
        self
    }

    fn measurement_name(&self) -> String {
        format!("{}/{}", self.workload, self.fixture_scale)
    }
}

#[must_use]
pub fn runner(tier: BenchmarkTier, suite: &'static str) -> CassieStressRunner {
    CassieStressRunner::new(suite, tier)
}

impl CassieStressRunner {
    /// # Panics
    ///
    /// Panics when the selected deployment profile is not registered or the benchmark
    /// configuration is invalid.
    #[must_use]
    pub fn new(suite: &'static str, tier: BenchmarkTier) -> Self {
        let resolved = resolve_config(tier);
        if resolved.print_config {
            print_config(suite, &resolved.config, resolved.filter.as_deref());
            std::process::exit(0);
        }

        let filtered_run = resolved.filter.is_some()
            || resolved.tier_filter.is_some()
            || resolved.config.profile == RunProfile::Smoke;
        let mut runner = StressRunner::with_config(suite, resolved.config.clone());
        runner.metadata("filtered_run", filtered_run);
        runner.metadata("owner_suite_complete", !filtered_run);
        runner.metadata("declared_tier", tier.number());
        if let Some(profile_id) = std::env::var("CASSIE_BENCH_DEPLOYMENT_PROFILE_ID")
            .ok()
            .filter(|value| !value.is_empty())
        {
            let profile = performance_benchmarks::deployment_profile_for_id(&profile_id)
                .unwrap_or_else(|| panic!("unknown benchmark deployment profile '{profile_id}'"));
            runner.metadata("deployment_profile_id", profile.profile_id);
            match profile.storage_mode {
                "midge_disk_apfs" | "midge_disk_native_linux" => {
                    std::env::set_var("CASSIE_STORAGE_MODE", "local");
                }
                "in_memory_midge_fallback" => {
                    std::env::set_var("CASSIE_STORAGE_MODE", "memory");
                }
                _ => {}
            }
        }
        if let Some(run_id) = std::env::var("CASSIE_BENCH_RUN_ID")
            .ok()
            .filter(|value| !value.is_empty())
        {
            runner.metadata("run_id", run_id);
        }
        if let Some(soak) = resolved.soak_duration {
            runner.metadata("soak_total_duration_seconds", soak.total.as_secs());
            runner.metadata(
                "soak_per_sample_duration_seconds",
                resolved.config.sample_duration.as_secs_f64(),
            );
            runner.metadata("soak_measured_samples", resolved.config.samples);
            runner.metadata("soak_duration_source", soak.source);
        }
        Self {
            suite,
            tier,
            config: resolved.config,
            filter: resolved.filter,
            tier_filter: resolved.tier_filter,
            baseline: resolved.baseline,
            runner,
            selected: 0,
            fixture_identities: FixtureIdentityTracker::default(),
            relative_p95_gates: Vec::new(),
        }
    }

    pub fn require_relative_p95(
        &mut self,
        candidate_scenario: &'static str,
        baseline_scenario: &'static str,
        maximum_ratio: f64,
    ) {
        self.relative_p95_gates.push(RelativeP95Gate::new(
            candidate_scenario,
            baseline_scenario,
            maximum_ratio,
        ));
    }

    /// Measures a Tier 1 production kernel.
    ///
    /// # Panics
    ///
    /// Panics when called by another tier or when the case violates the registry contract.
    pub fn measure_micro<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
        R: BenchmarkObservation,
    {
        self.require_tier(BenchmarkTier::Tier1, "measure_micro");
        let case = self.prepare_case(case, BenchmarkTimingMode::Micro);
        self.run_micro(case, f);
    }

    /// Measures a Tier 1 production kernel batch and normalizes it to one logical operation.
    ///
    /// # Panics
    ///
    /// Panics when called by another tier, when the batch is empty, or when the case violates the
    /// registry contract.
    pub fn measure_micro_batch<F, R>(&mut self, case: StressCase, logical_operations: u64, f: F)
    where
        F: FnMut() -> R,
        R: BenchmarkObservation,
    {
        self.require_tier(BenchmarkTier::Tier1, "measure_micro_batch");
        assert!(
            logical_operations != 0,
            "measure_micro_batch requires logical_operations > 0"
        );
        let case = self.prepare_case(case, BenchmarkTimingMode::Micro);
        self.run_micro_batch(case, logical_operations, f);
    }

    /// Measures one Tier 2 subsystem operation.
    ///
    /// # Panics
    ///
    /// Panics when called by another tier or when the case violates the registry contract.
    pub fn measure<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
        R: BenchmarkObservation,
    {
        self.require_tier(BenchmarkTier::Tier2, "measure");
        let case = self.prepare_case(case, BenchmarkTimingMode::Measure);
        self.run_measure(case, f);
    }

    /// Measures one Tier 2 subsystem operation that reports its completed count.
    ///
    /// # Panics
    ///
    /// Panics when called by another tier, when no operations complete, or when the case violates
    /// the registry contract.
    pub fn measure_counted<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
        R: CountedObservation,
    {
        self.require_tier(BenchmarkTier::Tier2, "measure_counted");
        let case = self.prepare_case(case, BenchmarkTimingMode::Counted);
        self.run_counted(case, f);
    }

    /// Measures a bounded batch of Tier 2 counted subsystem operations.
    ///
    /// The recorded elapsed time covers the whole batch while the completed-operation count keeps
    /// throughput normalized to the case's declared logical operation unit.
    ///
    /// # Panics
    ///
    /// Panics when called by another tier, when the batch is empty, when the completed count
    /// overflows `u64`, or when the case violates the registry contract.
    pub fn measure_counted_batch<F>(
        &mut self,
        case: StressCase,
        fixture_invocations: usize,
        mut f: F,
    ) where
        F: FnMut() -> u64,
    {
        self.require_tier(BenchmarkTier::Tier2, "measure_counted_batch");
        let case = case.parameter(
            "fixture_invocations_per_sample",
            fixture_invocations.to_string(),
        );
        self.measure_counted(case, move || {
            repeat_counted_batch(fixture_invocations, &mut f)
        });
    }

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
        assert!(
            matches!(
                self.tier,
                BenchmarkTier::Tier3
                    | BenchmarkTier::Tier4
                    | BenchmarkTier::Tier5
                    | BenchmarkTier::Tier6
            ),
            "measure_batch is only valid for Tiers 3-6"
        );
        let case = self.prepare_case(case, BenchmarkTimingMode::Batch);
        self.run_batch(case, logical_operations, f);
    }

    /// Records one sample timed by a real Tier 4 or Tier 6 external harness.
    ///
    /// # Panics
    ///
    /// Panics when called by another tier, when no operations complete, or when the case violates
    /// the registry contract.
    pub fn record_external<F>(&mut self, case: StressCase, f: F)
    where
        F: FnMut(Duration) -> ExternalSample,
    {
        assert!(
            matches!(self.tier, BenchmarkTier::Tier4 | BenchmarkTier::Tier6),
            "record_external is only valid for Tiers 4 and 6"
        );
        let f = RefCell::new(f);
        let sample_duration = self.config.sample_duration;
        let case = self.prepare_case(
            case.metadata("runtime_evidence_unit", "logical_operation"),
            BenchmarkTimingMode::External,
        );
        let declared_cardinality = declared_result_cardinality(&case);
        let evidence = case.runtime_evidence.clone();
        let preflight = case.preflight_evidence.clone();
        let scenario = self.scenario_for(&case);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let sample = (f.borrow_mut())(sample_duration);
            let elapsed = external_elapsed(sample.elapsed, sample.completed_operations);
            ctx.record_external(&measurement_name, elapsed, sample.completed_operations);
            ctx.metadata("failed_operations", 0);
            record_observed_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                RuntimeEvidenceObservation::new(
                    declared_cardinality.unwrap_or(sample.completed_operations),
                    None,
                    None,
                )
                .per_external_operation(sample.completed_operations),
            );
        });
    }

    fn run_micro<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
        R: BenchmarkObservation,
    {
        let f = RefCell::new(f);
        let declared_cardinality = declared_result_cardinality(&case);
        let evidence = case.runtime_evidence.clone();
        let preflight = case.preflight_evidence.clone();
        let scenario = self.scenario_for(&case);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let result = black_box(ctx.measure(&measurement_name, || (f.borrow_mut())()));
            ctx.metadata("failed_operations", 0);
            record_observed_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                RuntimeEvidenceObservation::new(
                    declared_cardinality.unwrap_or_else(|| result.cardinality()),
                    result.candidate_count(),
                    result.peak_query_memory_bytes(),
                ),
            );
        });
    }

    fn run_micro_batch<F, R>(&mut self, case: StressCase, logical_operations: u64, f: F)
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
            let last_cardinality = std::cell::Cell::new(0_u64);
            let last_candidate_count = std::cell::Cell::new(None);
            let last_peak_query_memory_bytes = std::cell::Cell::new(None);
            let completed = ctx.measure_batch(&measurement_name, logical_operations, || {
                let result = (f.borrow_mut())();
                last_cardinality.set(result.cardinality());
                last_candidate_count.set(result.candidate_count());
                last_peak_query_memory_bytes.set(result.peak_query_memory_bytes());
                black_box(result);
            });
            ctx.metadata("failed_operations", 0);
            record_observed_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                RuntimeEvidenceObservation::new(
                    declared_cardinality.unwrap_or_else(|| last_cardinality.get()),
                    last_candidate_count.get(),
                    last_peak_query_memory_bytes.get(),
                )
                .per_external_operation(completed),
            );
        });
    }

    fn run_measure<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
        R: BenchmarkObservation,
    {
        let f = RefCell::new(f);
        let declared_cardinality = declared_result_cardinality(&case);
        let evidence = case.runtime_evidence.clone();
        let preflight = case.preflight_evidence.clone();
        let scenario = self.scenario_for(&case);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let result = ctx.measure(&measurement_name, || black_box((f.borrow_mut())()));
            ctx.metadata("failed_operations", 0);
            record_observed_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                RuntimeEvidenceObservation::new(
                    declared_cardinality.unwrap_or_else(|| result.cardinality()),
                    result.candidate_count(),
                    result.peak_query_memory_bytes(),
                ),
            );
        });
    }

    fn run_counted<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
        R: CountedObservation,
    {
        let f = RefCell::new(f);
        let declared_cardinality = declared_result_cardinality(&case);
        let evidence = case.runtime_evidence.clone();
        let preflight = case.preflight_evidence.clone();
        let scenario = self.scenario_for(&case);
        let case = case
            .intent(MeasurementIntent::External)
            .metadata("logical_operations_source", "completed_count");
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let started = Instant::now();
            let observation = (f.borrow_mut())();
            let elapsed = started.elapsed();
            let completed = observation.completed_operations();
            ctx.record_external(&measurement_name, elapsed, completed);
            ctx.metadata("failed_operations", 0);
            record_observed_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                RuntimeEvidenceObservation::new(
                    declared_cardinality.unwrap_or_else(|| observation.result_cardinality()),
                    observation.candidate_count(),
                    observation.peak_query_memory_bytes(),
                ),
            );
            observation.finish_sample();
        });
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
            let last_cardinality = std::cell::Cell::new(0_u64);
            let last_candidate_count = std::cell::Cell::new(None);
            let last_peak_query_memory_bytes = std::cell::Cell::new(None);
            let completed = ctx.measure_batch(&measurement_name, logical_operations, || {
                let result = (f.borrow_mut())();
                last_cardinality.set(result.cardinality());
                last_candidate_count.set(result.candidate_count());
                last_peak_query_memory_bytes.set(result.peak_query_memory_bytes());
                black_box(result);
            });
            ctx.metadata("failed_operations", 0);
            record_observed_evidence(
                ctx,
                evidence.as_ref(),
                scenario,
                preflight.as_ref(),
                RuntimeEvidenceObservation::new(
                    declared_cardinality.unwrap_or_else(|| last_cardinality.get()),
                    last_candidate_count.get(),
                    last_peak_query_memory_bytes.get(),
                )
                .per_external_operation(completed),
            );
        });
    }

    #[must_use]
    pub fn is_enabled(&self, case: &StressCase) -> bool {
        let scenario = performance_benchmarks::expect_benchmark(
            self.suite,
            &case.workload,
            &case.fixture_scale,
        );
        let prepared = self.prepare_case(case.clone(), scenario.timing_mode);
        self.validate_registry_case(&prepared);
        if prepared.runtime_declaration.is_some() {
            self.validate_runtime_case(&prepared);
        }
        self.should_run(case)
    }

    /// Finishes the owner run and enforces its hard run gate.
    ///
    /// # Panics
    ///
    /// Panics when an evidence or resource gate fails, or when a selected baseline cannot be read.
    pub fn finish(self) {
        if self.selected == 0 {
            eprintln!("No stress benchmarks matched the selected filters.");
        }

        let relative_p95_gates = self.relative_p95_gates;
        let run = if let Some(baseline) = self.baseline {
            self.runner.finish_with_baseline(baseline)
        } else {
            Ok(self.runner.finish())
        };

        match run {
            Ok(run) => {
                let gate = evaluate_run_gate(&run);
                assert_eq!(gate, RunGate::Passed, "stress run gate failed: {gate:?}");
                validate_relative_p95_gates(&run, &relative_p95_gates)
                    .unwrap_or_else(|error| panic!("{error}"));
            }
            Err(error) => {
                panic!("stress run failed to load baseline: {error}");
            }
        }
    }

    fn run_case<F>(&mut self, case: StressCase, f: F)
    where
        F: Fn(&mut StressContext),
    {
        self.validate_registry_case(&case);
        self.validate_runtime_case(&case);
        if !self.should_run(&case) {
            return;
        }
        let fixture = case
            .runtime_declaration
            .as_ref()
            .expect("validated runtime declaration")
            .fixture();
        self.fixture_identities
            .register(self.suite, &case.fixture_scale, fixture.identity())
            .unwrap_or_else(|error| panic!("invalid benchmark fixture reuse: {error}"));
        let scenario = self.scenario_for(&case);
        stress_evidence::validate_preflight_requirement(scenario, case.preflight_evidence.as_ref())
            .unwrap_or_else(|error| panic!("invalid benchmark evidence: {error}"));

        self.selected = self.selected.saturating_add(1);
        let spec = self.spec_for(case);
        self.runner.run_spec(&spec, f);
    }

    fn prepare_case(&self, mut case: StressCase, timing_mode: BenchmarkTimingMode) -> StressCase {
        case.tier = self.tier.number();
        case.mode = match self.tier {
            BenchmarkTier::Tier1 => BenchmarkModeKind::Micro,
            BenchmarkTier::Tier2 => BenchmarkModeKind::FixedOperations,
            BenchmarkTier::Tier3
            | BenchmarkTier::Tier4
            | BenchmarkTier::Tier5
            | BenchmarkTier::Tier6 => BenchmarkModeKind::FixedDuration,
        };
        case.intent = match timing_mode {
            BenchmarkTimingMode::Micro | BenchmarkTimingMode::Measure => MeasurementIntent::General,
            BenchmarkTimingMode::Counted | BenchmarkTimingMode::External => {
                MeasurementIntent::External
            }
            BenchmarkTimingMode::Batch => MeasurementIntent::Batch,
        };
        case
    }

    fn require_tier(&self, expected: BenchmarkTier, method: &str) {
        assert_eq!(
            self.tier,
            expected,
            "{method} is only valid for Tier {}",
            expected.number()
        );
    }

    fn validate_registry_case(&self, case: &StressCase) {
        let scenario = performance_benchmarks::expect_benchmark(
            self.suite,
            &case.workload,
            &case.fixture_scale,
        );
        performance_benchmarks::validate_scenario_contract(scenario)
            .unwrap_or_else(|error| panic!("invalid benchmark scenario: {error}"));
        assert_eq!(
            scenario.declared_tier,
            self.tier,
            "scenario {} belongs to Tier {}, runner is Tier {}",
            scenario.scenario_id,
            scenario.declared_tier.number(),
            self.tier.number()
        );
        assert_eq!(
            case.tier,
            self.tier.number(),
            "scenario {} case tier disagrees with its runner",
            scenario.scenario_id
        );
        let actual_timing = timing_mode_for_case(case, self.tier);
        assert_eq!(
            scenario.timing_mode, actual_timing,
            "scenario {} timing mode mismatch",
            scenario.scenario_id
        );
    }

    fn validate_runtime_case(&self, case: &StressCase) {
        let scenario = self.scenario_for(case);
        validate_runtime_case_contract(scenario, case.runtime_declaration.as_ref())
            .unwrap_or_else(|error| panic!("invalid benchmark runtime case: {error}"));
    }

    fn scenario_for(
        &self,
        case: &StressCase,
    ) -> &'static performance_benchmarks::PerformanceBenchmarkScenario {
        performance_benchmarks::expect_benchmark(self.suite, &case.workload, &case.fixture_scale)
    }

    fn should_run(&self, case: &StressCase) -> bool {
        if self
            .tier_filter
            .is_some_and(|tier| tier != self.tier.number())
        {
            return false;
        }

        self.filter.as_deref().is_none_or(|filter| {
            let scenario = performance_benchmarks::benchmark_for_benchmark(
                self.suite,
                &case.workload,
                &case.fixture_scale,
            );
            let mut candidates = vec![
                self.suite.to_string(),
                case.workload.clone(),
                case.fixture_scale.clone(),
                format!("{}/{}", case.workload, case.fixture_scale),
                format!("{}/{}/{}", self.suite, case.workload, case.fixture_scale),
            ];
            if let Some(scenario) = scenario {
                candidates.push(scenario.scenario_id.to_string());
                candidates.push(scenario.family.to_string());
            }
            candidates
                .iter()
                .any(|candidate| matches_filter(candidate, filter))
        })
    }

    fn spec_for(&self, case: StressCase) -> BenchmarkSpec {
        let scenario = performance_benchmarks::expect_benchmark(
            self.suite,
            &case.workload,
            &case.fixture_scale,
        );
        let fixture_identity = case
            .runtime_declaration
            .as_ref()
            .expect("validated runtime declaration")
            .fixture()
            .identity()
            .to_string();
        let mut metadata = case.metadata;
        metadata.insert("benchmark".to_string(), self.suite.to_string());
        metadata.insert("workload".to_string(), case.workload.clone());
        metadata.insert("fixture_scale".to_string(), case.fixture_scale.clone());
        metadata.insert("scenario_id".to_string(), scenario.scenario_id.to_string());
        metadata.insert("family".to_string(), scenario.family.to_string());
        metadata.insert(
            "access_family".to_string(),
            scenario.access_family.to_string(),
        );
        metadata.insert(
            "signal_role".to_string(),
            scenario.evidence_role.signal_role().to_string(),
        );
        metadata.insert(
            "operation_unit".to_string(),
            scenario.operation_unit.to_string(),
        );
        metadata.insert(
            "fixture_class".to_string(),
            format!("{:?}", scenario.fixture_class).to_ascii_lowercase(),
        );
        metadata.insert(
            "fixture_rows".to_string(),
            scenario.fixture_rows.to_string(),
        );
        metadata.insert("fixture_identity".to_string(), fixture_identity);
        if let Some(client_count) = scenario.client_count {
            metadata.insert("client_count".to_string(), client_count.to_string());
        }
        if let Some(worker_count) = scenario.worker_count {
            metadata.insert("worker_count".to_string(), worker_count.to_string());
        }
        metadata.insert(
            "configured_worker_count".to_string(),
            scenario.worker_count.map_or(0, u16::from).to_string(),
        );
        metadata
            .entry("setup_time_ns".to_string())
            .or_insert_with(|| "0".to_string());

        let id = format!("{}/{}/{}", self.suite, case.workload, case.fixture_scale);
        BenchmarkSpec {
            id,
            name: format!("{}/{}", case.workload, case.fixture_scale),
            tier: self.tier.number(),
            mode: self
                .config
                .mode_for_tier(self.tier.number())
                .unwrap_or_else(|| self.config.mode_for_kind(case.mode)),
            intent: case.intent,
            budgets: BenchmarkBudgets::default(),
            parameters: case.parameters,
            metadata,
        }
    }
}

/// A benchmark result whose observed cardinality can be written to the artifact.
pub trait BenchmarkObservation {
    /// Returns the cardinality produced by the final timed operation.
    fn cardinality(&self) -> u64;

    /// Returns an observed candidate count when the operation has candidates.
    fn candidate_count(&self) -> Option<u64> {
        None
    }

    /// Returns observed peak query memory when the kernel uses query controls.
    fn peak_query_memory_bytes(&self) -> Option<u64> {
        None
    }
}

impl BenchmarkObservation for usize {
    fn cardinality(&self) -> u64 {
        u64::try_from(*self).expect("benchmark observation should fit u64")
    }
}

impl BenchmarkObservation for u64 {
    fn cardinality(&self) -> u64 {
        *self
    }
}

impl BenchmarkObservation for cassie::benchmark::KernelObservation {
    fn cardinality(&self) -> u64 {
        self.result_cardinality()
    }

    fn candidate_count(&self) -> Option<u64> {
        self.candidate_count()
    }

    fn peak_query_memory_bytes(&self) -> Option<u64> {
        self.peak_query_memory_bytes()
    }
}

pub trait CountedObservation {
    fn completed_operations(&self) -> u64;
    fn result_cardinality(&self) -> u64;
    fn candidate_count(&self) -> Option<u64>;
    fn peak_query_memory_bytes(&self) -> Option<u64>;
    fn finish_sample(self);
}

impl CountedObservation for u64 {
    fn completed_operations(&self) -> u64 {
        *self
    }

    fn result_cardinality(&self) -> u64 {
        *self
    }

    fn candidate_count(&self) -> Option<u64> {
        None
    }

    fn peak_query_memory_bytes(&self) -> Option<u64> {
        None
    }

    fn finish_sample(self) {}
}

impl CountedObservation for cassie::benchmark::KernelObservation {
    fn completed_operations(&self) -> u64 {
        self.completed_operations()
    }

    fn result_cardinality(&self) -> u64 {
        self.result_cardinality()
    }

    fn candidate_count(&self) -> Option<u64> {
        self.candidate_count()
    }

    fn peak_query_memory_bytes(&self) -> Option<u64> {
        self.peak_query_memory_bytes()
    }

    fn finish_sample(self) {
        cassie::benchmark::KernelObservation::finish_sample(self);
    }
}

fn record_observed_evidence(
    context: &mut StressContext,
    source: Option<&RuntimeEvidenceSource>,
    scenario: &performance_benchmarks::PerformanceBenchmarkScenario,
    preflight: Option<&PreflightEvidence>,
    observation: RuntimeEvidenceObservation,
) {
    if let Some(source) = source {
        source.record(context, scenario, preflight, observation);
    } else {
        stress_evidence::record_without_runtime(context, scenario, preflight, observation);
    }
}

fn declared_result_cardinality(case: &StressCase) -> Option<u64> {
    case.metadata
        .get("result_cardinality")
        .and_then(|value| value.parse().ok())
}

fn prepare_batch_case(case: StressCase, logical_operations: u64) -> StressCase {
    let logical_unit = case
        .runtime_declaration
        .as_ref()
        .expect("validated runtime declaration")
        .operation_unit()
        .as_str();
    case.intent(MeasurementIntent::Batch)
        .parameter("logical_unit", logical_unit)
        .parameter(
            "logical_operations_per_iteration",
            logical_operations.to_string(),
        )
        .metadata(
            "logical_operations_per_iteration",
            logical_operations.to_string(),
        )
        .metadata("measurement_shape", "fixed_workload")
}

pub(crate) fn repeat_counted_batch<F>(fixture_invocations: usize, mut f: F) -> u64
where
    F: FnMut() -> u64,
{
    assert!(
        fixture_invocations > 0,
        "counted batch requires at least one fixture invocation"
    );
    (0..fixture_invocations).fold(0_u64, |completed, _| {
        completed
            .checked_add(f())
            .expect("counted batch completed-operation count should fit u64")
    })
}

fn timing_mode_for_case(case: &StressCase, tier: BenchmarkTier) -> BenchmarkTimingMode {
    match (case.mode, case.intent) {
        (BenchmarkModeKind::Micro, _) => BenchmarkTimingMode::Micro,
        (_, MeasurementIntent::Batch) => BenchmarkTimingMode::Batch,
        (_, MeasurementIntent::External) if tier == BenchmarkTier::Tier2 => {
            BenchmarkTimingMode::Counted
        }
        (_, MeasurementIntent::External) => BenchmarkTimingMode::External,
        _ => BenchmarkTimingMode::Measure,
    }
}
