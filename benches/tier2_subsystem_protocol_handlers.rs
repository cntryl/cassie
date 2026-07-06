#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const HANDLER_BATCH: usize = 128;
const PREPARED_LOOP_BATCH: usize = 512;
const PREPARED_LOOP_MESSAGES: usize = 5;
const JSON_SERIALIZATION_BATCH: usize = 16;
const JSON_SERIALIZATION_ROWS: usize = 512;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-protocol-handlers", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier2_subsystem_protocol_handlers");
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "postgres_wire_handler", "10k")
            .metadata("operation_unit", "query"),
        logical_operations(HANDLER_BATCH),
        || {
            repeat(HANDLER_BATCH, || {
                runtime.block_on(workloads::pgwire_simple_query(
                    &ctx,
                    "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20",
                ))
            })
        },
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "http_handler", "10k")
            .metadata("operation_unit", "document_get_request"),
        logical_operations(HANDLER_BATCH),
        || {
            repeat(HANDLER_BATCH, || {
                runtime.block_on(workloads::http_document_get(&ctx))
            })
        },
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "prepared_statement_loop", "protocol")
            .metadata("operation_unit", "protocol_message"),
        logical_operations(PREPARED_LOOP_BATCH * PREPARED_LOOP_MESSAGES),
        || {
            repeat(
                PREPARED_LOOP_BATCH,
                workloads::pgwire_prepared_statement_protocol_loop,
            )
        },
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "json_serialization_overhead", "512_rows")
            .metadata("operation_unit", "serialized_row"),
        logical_operations(JSON_SERIALIZATION_BATCH * JSON_SERIALIZATION_ROWS),
        || {
            repeat(
                JSON_SERIALIZATION_BATCH,
                workloads::json_serialization_overhead,
            )
        },
    );
    runner.finish();
}

fn repeat(mut iterations: usize, mut f: impl FnMut() -> usize) -> usize {
    let mut completed = 0usize;
    while iterations > 0 {
        completed = completed.saturating_add(f());
        iterations -= 1;
    }
    std::hint::black_box(completed)
}

fn logical_operations(iterations: usize) -> u64 {
    u64::try_from(iterations).expect("benchmark batch size should fit u64")
}
