#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-protocol-handlers", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier2_subsystem_protocol_handlers");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "postgres_wire_handler", "10k"),
        || {
            runtime.block_on(workloads::pgwire_simple_query(
                &ctx,
                "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20",
            ))
        },
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "http_handler", "10k"),
        || runtime.block_on(workloads::http_document_get(&ctx)),
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "prepared_statement_loop", "protocol")
            .metadata("operation_unit", "protocol_message"),
        5,
        workloads::pgwire_prepared_statement_protocol_loop,
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "json_serialization_overhead", "512_rows")
            .metadata("operation_unit", "serialized_row"),
        512,
        workloads::json_serialization_overhead,
    );
    runner.finish();
}
