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
    let http = runtime
        .block_on(workloads::http_transport_context(&ctx))
        .expect("http transport context");
    let pgwire = runtime
        .block_on(workloads::pgwire_transport_for_context(&ctx))
        .expect("pgwire transport context");

    let mut runner = stress::runner("tier4_integration_protocol_compare");
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(4, "postgres_wire_query", "10k")
            .metadata("operation_unit", "result_row"),
        20,
        || {
            runtime.block_on(workloads::pgwire_transport_simple_query(
                &pgwire,
                "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20",
            ))
        },
    );
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(4, "http_json_query", "10k")
            .metadata("operation_unit", "document_get_request"),
        20,
        || runtime.block_on(workloads::http_transport_document_get_batch(&http, 20)),
    );
    runner.finish();
}
