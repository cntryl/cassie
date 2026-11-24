use std::time::Instant;

const BENCHMARK: &str = "tier3_system_startup";
const FIXTURE_ROWS: usize = 100_000;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier3, BENCHMARK);
    let case = stress::StressCase::new("startup_reopen", "100k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Representative,
            FIXTURE_ROWS,
            "tier3_system_startup/100k",
        ),
        stress::OperationUnit::Startup,
    );
    if !runner.is_enabled(&case) {
        runner.finish();
        return;
    }

    workloads::configure_tier3_environment();
    let setup_started = Instant::now();
    let fixture = workloads::StartupFixture::new("tier3-startup-100k", FIXTURE_ROWS)
        .expect("prebuilt Tier 3 startup fixture");
    let case = case.metadata(
        "setup_time_ns",
        setup_started.elapsed().as_nanos().to_string(),
    );
    runner.measure_batch(case, 1, || fixture.reopen());
    runner.finish();
}
