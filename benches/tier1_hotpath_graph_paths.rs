//! Tier 1 kernels of the mapped graph path values. No storage is involved.
#[path = "support/graph_kernel.rs"]
mod graph_kernel;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;

use std::time::Instant;

const BENCHMARK: &str = "tier1_hotpath_graph_paths";

fn kernel_case(workload: &'static str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            format!("{BENCHMARK}/micro"),
        ),
        unit,
    )
}

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier1, BENCHMARK);
    let value_case = kernel_case("graph_path_value", stress::OperationUnit::Operation);
    let order_case = kernel_case("graph_path_order", stress::OperationUnit::Comparison);
    let key_case = kernel_case("graph_key_normalization", stress::OperationUnit::Key);
    if !runner.is_enabled(&value_case)
        && !runner.is_enabled(&order_case)
        && !runner.is_enabled(&key_case)
    {
        runner.finish();
        return;
    }

    let setup_started = Instant::now();
    let kernel = graph_kernel::GraphPathKernel::new();
    let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();
    if runner.is_enabled(&value_case) {
        runner.measure_micro_batch(
            value_case.metadata("setup_time_ns", setup_time_ns.clone()),
            kernel.paths_per_batch(),
            || kernel.construct_and_validate(),
        );
    }
    if runner.is_enabled(&order_case) {
        runner.measure_micro_batch(
            order_case.metadata("setup_time_ns", setup_time_ns.clone()),
            kernel.comparisons_per_batch(),
            || kernel.compare_neighbours(),
        );
    }
    if runner.is_enabled(&key_case) {
        runner.measure_micro_batch(
            key_case.metadata("setup_time_ns", setup_time_ns),
            kernel.key_tuple_len(),
            || kernel.normalize_key_tuple(),
        );
    }
    runner.finish();
}
