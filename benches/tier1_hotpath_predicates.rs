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
        "tier1_hotpath_predicates",
    );
    let predicate_case = stress::StressCase::new("predicate_evaluation", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_predicates/micro",
        ),
        stress::OperationUnit::Predicate,
    );
    if runner.is_enabled(&predicate_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("predicate_evaluation").expect("registered Tier 1 workload");
        let predicate_case = predicate_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(predicate_case, workloads::predicate_evaluation);
    }
    runner.finish();
}
