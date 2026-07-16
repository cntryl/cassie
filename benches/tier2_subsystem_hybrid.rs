#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_hybrid",
    );
    let case = stress::StressCase::new("hybrid_fusion", "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            2_048,
            "tier2_subsystem_hybrid/2k",
        ),
        stress::OperationUnit::Candidate,
    );
    if runner.is_enabled(&case) {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::HybridFusionFixture::new(2_048);
        runner.measure_counted(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().to_string(),
            ),
            || fixture.fuse(),
        );
    }
    runner.finish();
}
