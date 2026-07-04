#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_keys");
    runner.tier1_micro("key_encode_decode", workloads::key_encode_decode);
    runner.tier1_micro("field_lookup", workloads::field_lookup);
    runner.finish();
}
