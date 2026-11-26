use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::benchmark::ExecutorKernel;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_filter_projection",
    );
    let setup_started = Instant::now();
    let kernel = ExecutorKernel::sample();
    let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();
    let filter = case("batch_filter", stress::OperationUnit::Batch);
    if runner.is_enabled(&filter) {
        runner.measure_micro(filter.metadata("setup_time_ns", &setup_time_ns), || {
            std::hint::black_box(kernel.filter_batch())
        });
    }
    let projection = case("batch_projection", stress::OperationUnit::Row);
    if runner.is_enabled(&projection) {
        runner.measure_micro_batch(
            projection.metadata("setup_time_ns", &setup_time_ns),
            64,
            || {
                for _ in 0..64 {
                    std::hint::black_box(kernel.project_row());
                }
                64_u64
            },
        );
    }
    let comparison = case("value_comparison", stress::OperationUnit::Comparison);
    if runner.is_enabled(&comparison) {
        runner.measure_micro_batch(
            comparison.metadata("setup_time_ns", &setup_time_ns),
            256,
            || {
                for _ in 0..256 {
                    std::hint::black_box(kernel.value_comparison_matches());
                }
                256_u64
            },
        );
    }
    runner.finish();
}

fn case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_filter_projection/micro",
        ),
        unit,
    )
}
