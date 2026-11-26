#[path = "support/observation.rs"]
mod observation;

use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::benchmark::KernelObservation;
use observation::ObservationBridge;

const ROWS: usize = 2_048;
const FUSIONS_PER_SAMPLE: usize = 256;
const SUITE: &str = "tier2_subsystem_hybrid";

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier2, SUITE);
    let case = stress::StressCase::new("hybrid_fusion", "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            ROWS,
            "tier2_subsystem_hybrid/2k",
        ),
        stress::OperationUnit::Candidate,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        let fixture = HybridFusionFixture::new(ROWS);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        runner.measure_counted(
            case.metadata("setup_time_ns", setup_time_ns).parameter(
                "fixture_invocations_per_sample",
                FUSIONS_PER_SAMPLE.to_string(),
            ),
            || ObservationBridge(fixture.fuse_batch(FUSIONS_PER_SAMPLE)),
        );
    }
    runner.finish();
}

struct HybridFusionFixture {
    scores: Vec<(f64, f64)>,
}

impl HybridFusionFixture {
    fn new(rows: usize) -> Self {
        assert!(
            (1..=2_048).contains(&rows),
            "Tier 2 fixture rows must be in 1..=2,048"
        );
        let scores = (0..rows)
            .map(|index| {
                let search = usize_to_f64(index % 100) / 100.0;
                let vector = usize_to_f64((rows - index) % 100) / 100.0;
                (search, vector)
            })
            .collect();
        Self { scores }
    }

    fn fuse_batch(&self, fixture_invocations: usize) -> KernelObservation {
        assert!(
            fixture_invocations > 0,
            "hybrid-fusion batch must not be empty"
        );
        let completed = (0..fixture_invocations).fold(0_u64, |total, _| {
            total
                .checked_add(self.fuse_candidate_count())
                .expect("hybrid-fusion candidate count fits u64")
        });
        KernelObservation::new(completed, completed).with_candidate_count(completed)
    }

    fn fuse_candidate_count(&self) -> u64 {
        let total = self
            .scores
            .iter()
            .map(|(search, vector)| cassie_engine::hybrid::hybrid_score(*search, *vector, None))
            .sum::<f64>();
        std::hint::black_box(total);
        u64::try_from(self.scores.len()).expect("benchmark candidate count fits u64")
    }
}

fn usize_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("benchmark value should fit u32"))
}
