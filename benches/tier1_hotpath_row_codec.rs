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
        "tier1_hotpath_row_codec",
    );
    let case = stress::StressCase::new("row_encode_decode", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_row_codec/micro",
        ),
        stress::OperationUnit::Row,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("row_encode_decode").expect("registered Tier 1 workload");
        let case = case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(case, workloads::row_encode_decode);
    }
    runner.finish();
}
