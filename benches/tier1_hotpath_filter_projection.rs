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
        "tier1_hotpath_filter_projection",
    );
    let filter_case = declared_case("batch_filter", stress::OperationUnit::Batch);
    if runner.is_enabled(&filter_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("batch_filter").expect("registered Tier 1 workload");
        let filter_case = filter_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(filter_case, workloads::batch_filter);
    }

    let projection_case = declared_case("batch_projection", stress::OperationUnit::Row);
    if runner.is_enabled(&projection_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("batch_projection").expect("registered Tier 1 workload");
        let projection_case = projection_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(projection_case, workloads::batch_projection);
    }

    let comparison_case = declared_case("value_comparison", stress::OperationUnit::Comparison);
    if runner.is_enabled(&comparison_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("value_comparison").expect("registered Tier 1 workload");
        let comparison_case = comparison_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(comparison_case, workloads::value_comparison);
    }
    runner.finish();
}

fn declared_case(workload: &str, operation_unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_filter_projection/micro",
        ),
        operation_unit,
    )
}
