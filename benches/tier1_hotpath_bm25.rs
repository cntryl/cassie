#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_bm25");
    runner.micro(
        stress::StressCase::tier1_micro("bm25_scoring")
            .metadata("logical_operations_per_iteration", "8"),
        workloads::bm25_score,
    );
    runner.finish();
}
