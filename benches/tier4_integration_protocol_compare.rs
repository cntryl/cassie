#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier4-protocol-compare", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier4_integration_protocol_compare");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(4, "direct_query_baseline", "10k"),
        || runtime.block_on(workloads::protocol_comparison_sql(&ctx)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(4, "postgres_wire_query", "10k"),
        || runtime.block_on(workloads::protocol_comparison_pgwire(&ctx)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(4, "http_json_query", "10k"),
        || runtime.block_on(workloads::protocol_comparison_http(&ctx)),
    );
    runner.finish();
}
