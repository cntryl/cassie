#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_bm25");
    runner.tier1_micro("bm25_scoring", workloads::bm25_score);
    runner.finish();
}
