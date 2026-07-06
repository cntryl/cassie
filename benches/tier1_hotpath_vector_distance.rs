#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_vector_distance");
    runner.micro(
        stress::StressCase::tier1_micro("cosine_distance")
            .metadata("logical_operations_per_iteration", "32"),
        workloads::cosine_distance,
    );
    runner.micro(
        stress::StressCase::tier1_micro("dot_product")
            .metadata("logical_operations_per_iteration", "32"),
        workloads::dot_product,
    );
    runner.micro(
        stress::StressCase::tier1_micro("l2_distance")
            .metadata("logical_operations_per_iteration", "32"),
        workloads::l2_distance,
    );
    runner.finish();
}
