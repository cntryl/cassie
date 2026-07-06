#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_row_codec");
    runner.tier1_micro("row_encode_decode", workloads::row_encode_decode);
    runner.finish();
}
