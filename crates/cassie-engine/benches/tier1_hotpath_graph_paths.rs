#[path = "support/graph_kernel.rs"]
mod graph_kernel;

use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};

const SUITE: &str = "tier1_hotpath_graph_paths";

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier1, SUITE);
    let cases = [
        ("graph_path_value", stress::OperationUnit::Operation),
        ("graph_path_order", stress::OperationUnit::Comparison),
        ("graph_key_normalization", stress::OperationUnit::Key),
    ]
    .map(|(workload, unit)| declared_case(workload, unit));
    if !cases.iter().any(|case| runner.is_enabled(case)) {
        runner.finish();
        return;
    }
    let setup_started = Instant::now();
    let kernel = graph_kernel::GraphPathKernel::new();
    let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();
    if runner.is_enabled(&cases[0]) {
        runner.measure_micro_batch(
            cases[0].clone().metadata("setup_time_ns", &setup_time_ns),
            kernel.paths_per_batch(),
            || kernel.construct_and_validate(),
        );
    }
    if runner.is_enabled(&cases[1]) {
        runner.measure_micro_batch(
            cases[1].clone().metadata("setup_time_ns", &setup_time_ns),
            kernel.comparisons_per_batch(),
            || kernel.compare_neighbours(),
        );
    }
    if runner.is_enabled(&cases[2]) {
        runner.measure_micro_batch(
            cases[2].clone().metadata("setup_time_ns", &setup_time_ns),
            kernel.key_tuple_len(),
            || kernel.normalize_key_tuple(),
        );
    }
    runner.finish();
}

fn declared_case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            format!("{SUITE}/micro"),
        ),
        unit,
    )
}
