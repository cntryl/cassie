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
        "tier1_hotpath_vector_distance",
    );
    let cosine_case = declared_case("cosine_distance");
    if runner.is_enabled(&cosine_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("cosine_distance").expect("registered Tier 1 workload");
        let cosine_case = cosine_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(cosine_case, workloads::cosine_distance);
    }

    let dot_case = declared_case("dot_product");
    if runner.is_enabled(&dot_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("dot_product").expect("registered Tier 1 workload");
        let dot_case = dot_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(dot_case, workloads::dot_product);
    }

    let l2_case = declared_case("l2_distance");
    if runner.is_enabled(&l2_case) {
        let setup_started = Instant::now();
        workloads::prepare_hotpath("l2_distance").expect("registered Tier 1 workload");
        let l2_case = l2_case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro(l2_case, workloads::l2_distance);
    }
    runner.finish();
}

fn declared_case(workload: &str) -> stress::StressCase {
    stress::StressCase::new(workload, "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_vector_distance/micro",
        ),
        stress::OperationUnit::Distance,
    )
}
