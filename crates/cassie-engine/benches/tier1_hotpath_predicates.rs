use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::benchmark::ExecutorKernel;

const OPERATIONS_PER_SAMPLE: u64 = 256;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_predicates",
    );
    let case = stress::StressCase::new("predicate_evaluation", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_predicates/micro",
        ),
        stress::OperationUnit::Predicate,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        let kernel = ExecutorKernel::sample();
        let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();
        runner.measure_micro_batch(
            case.metadata("setup_time_ns", setup_time_ns),
            OPERATIONS_PER_SAMPLE,
            || {
                for _ in 0..OPERATIONS_PER_SAMPLE {
                    std::hint::black_box(kernel.predicate_matches());
                }
                OPERATIONS_PER_SAMPLE
            },
        );
    }
    runner.finish();
}
