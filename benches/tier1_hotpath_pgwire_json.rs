#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_pgwire_json");
    runner.tier1_micro("query_parameter_binding", workloads::parameter_binding);
    runner.tier1_micro("row_to_pgwire_encoding", workloads::row_to_pgwire_encoding);
    runner.tier1_micro("row_to_json_encoding", workloads::row_to_json_encoding);
    runner.finish();
}
