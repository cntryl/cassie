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
        "tier1_hotpath_keys",
    );
    let key_case = stress::StressCase::new("key_encode_decode", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_keys/micro",
        ),
        stress::OperationUnit::Key,
    );
    if runner.is_enabled(&key_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("key_encode_decode").expect("registered Tier 1 workload");
        let key_case = key_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(key_case, workloads::key_encode_decode);
    }
    runner.finish();
}
