#[path = "support/observation.rs"]
mod observation;
#[path = "support/subsystem.rs"]
mod workloads;

use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use observation::ObservationBridge;

const ROWS: usize = 2_048;
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
        let fixture = workloads::HybridFusionFixture::new(ROWS);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        runner.measure_counted(
            case.metadata("setup_time_ns", setup_time_ns).parameter(
                "fixture_invocations_per_sample",
                workloads::HYBRID_FUSION_INVOCATIONS_PER_SAMPLE.to_string(),
            ),
            || {
                ObservationBridge(
                    fixture.fuse_batch(workloads::HYBRID_FUSION_INVOCATIONS_PER_SAMPLE),
                )
            },
        );
    }
    runner.finish();
}
