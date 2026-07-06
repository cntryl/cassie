#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const PGWIRE_QUERY_BATCH: usize = 512;
const PGWIRE_WARMUP_BATCHES: usize = 5;
const PGWIRE_RESULT_ROWS: usize = 20;
const HTTP_REQUEST_BATCH: usize = 128;
const HTTP_WARMUP_BATCHES: usize = 4;

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
    let pgwire_sql = "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20";
    for _ in 0..PGWIRE_WARMUP_BATCHES {
        let _ = pgwire_simple_query_batch(&runtime, &pgwire, pgwire_sql, PGWIRE_QUERY_BATCH);
    }
    for _ in 0..HTTP_WARMUP_BATCHES {
        let _ = runtime.block_on(workloads::http_transport_document_get_batch(
            &http,
            HTTP_REQUEST_BATCH,
        ));
    }
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(4, "postgres_wire_query", "10k")
            .metadata("operation_unit", "result_row"),
        logical_operations(PGWIRE_QUERY_BATCH * PGWIRE_RESULT_ROWS),
        || pgwire_simple_query_batch(&runtime, &pgwire, pgwire_sql, PGWIRE_QUERY_BATCH),
    );
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(4, "http_json_query", "10k")
            .metadata("operation_unit", "document_get_request"),
        logical_operations(HTTP_REQUEST_BATCH),
        || {
            runtime.block_on(workloads::http_transport_document_get_batch(
                &http,
                HTTP_REQUEST_BATCH,
            ))
        },
    );
    runner.finish();
}

fn pgwire_simple_query_batch(
    runtime: &tokio::runtime::Runtime,
    ctx: &workloads::PgwireTransportBenchContext,
    sql: &str,
    queries: usize,
) -> usize {
    let mut rows = 0usize;
    for _ in 0..queries {
        rows = rows
            .saturating_add(runtime.block_on(workloads::pgwire_transport_simple_query(ctx, sql)));
    }
    std::hint::black_box(rows)
}

fn logical_operations(operations: usize) -> u64 {
    u64::try_from(operations).expect("benchmark operation count should fit u64")
}
