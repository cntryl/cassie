#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_topk");
    runner.tier1_micro("top_k_heap_maintenance", workloads::top_k_update);
    runner.finish();
}
