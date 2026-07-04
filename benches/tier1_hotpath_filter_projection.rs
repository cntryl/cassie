#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_filter_projection");
    runner.tier1_micro("predicate_evaluation", workloads::predicate_evaluation);
    runner.tier1_micro("batch_filter", workloads::batch_filter);
    runner.tier1_micro("batch_projection", workloads::batch_projection);
    runner.tier1_micro("value_comparison", workloads::value_comparison);
    runner.tier1_micro("top_k_update", workloads::top_k_update);
    runner.finish();
}
