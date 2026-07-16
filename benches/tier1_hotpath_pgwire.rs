#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

use std::time::Instant;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_pgwire",
    );
    let pgwire_case = stress::StressCase::new("row_to_pgwire_encoding", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_pgwire/micro",
        ),
        stress::OperationUnit::Row,
    );
    if runner.is_enabled(&pgwire_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("row_to_pgwire_encoding").expect("registered Tier 1 workload");
        let pgwire_case = pgwire_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(pgwire_case, workloads::row_to_pgwire_encoding);
    }
    runner.finish();
}
