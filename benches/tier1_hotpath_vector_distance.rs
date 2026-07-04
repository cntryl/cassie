#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_vector_distance");
    runner.tier1_micro("cosine_distance", workloads::cosine_distance);
    runner.tier1_micro("dot_product", workloads::dot_product);
    runner.tier1_micro("l2_distance", workloads::l2_distance);
    runner.finish();
}
