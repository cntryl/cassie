#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_parser",
    );
    let case = stress::StressCase::new("sql_parser", "128").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            128,
            "tier2_subsystem_parser/128",
        ),
        stress::OperationUnit::Statement,
    );
    if runner.is_enabled(&case) {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::ParserFixture::new(128);
        runner.measure_counted(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().to_string(),
            ),
            || fixture.parse(),
        );
    }
    runner.finish();
}
