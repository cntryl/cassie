const BENCHMARK: &str = "tier4_integration_pgwire";
const SIMPLE_QUERY_BATCH: usize = 512;
const SIMPLE_QUERY_WARMUP_BATCHES: usize = 2;
const SIMPLE_QUERY_ROWS: usize = 20;
const MULTI_STATEMENT_QUERY_BATCH: usize = 128;
const MULTI_STATEMENT_QUERY_ROWS: usize = 60;
const PREPARED_QUERY_BATCH: usize = 1_024;
const PREPARED_QUERY_ROWS: usize = 25;
const CONNECTION_CHURN_BATCH: usize = 32;
const LARGE_RESULT_ROWS: usize = 512;
const CONCURRENT_CLIENTS: usize = 8;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    bench_simple_query(&mut runner, &runtime);
    bench_multi_statement_query(&mut runner, &runtime);
    bench_prepared_query(&mut runner, &runtime);
    bench_legacy_rows(&mut runner, &runtime);

    runner.finish();
}

fn bench_multi_statement_query(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
) {
    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let benchmark = performance_benchmarks::expect_benchmark(
            BENCHMARK,
            "pgwire_multi_statement_query",
            dataset,
        );
        let case =
            stress::StressCase::fixed_operations(4, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let ctx = runtime
            .block_on(workloads::pgwire_transport_context(
                &format!("tier4-pgwire-multi-{dataset}"),
                rows,
            ))
            .expect("multi-statement benchmark context");
        let sql = "SELECT id, title FROM bench_documents WHERE title = 'title-1' ORDER BY id ASC LIMIT 20; SELECT id, title FROM bench_documents WHERE title = 'title-1' ORDER BY id ASC LIMIT 20; SELECT id, title FROM bench_documents WHERE title = 'title-1' ORDER BY id ASC LIMIT 20";
        let expected_rows = MULTI_STATEMENT_QUERY_BATCH * MULTI_STATEMENT_QUERY_ROWS;
        for _ in 0..SIMPLE_QUERY_WARMUP_BATCHES {
            let measured_rows =
                pgwire_multi_statement_query_batch(runtime, &ctx, sql, MULTI_STATEMENT_QUERY_BATCH);
            assert_eq!(
                measured_rows, expected_rows,
                "multi-statement result cardinality"
            );
        }
        runner.fixed_timed_count(
            case.metadata("operation_unit", "result_row"),
            logical_operations(expected_rows),
            || {
                let measured_rows = pgwire_multi_statement_query_batch(
                    runtime,
                    &ctx,
                    sql,
                    MULTI_STATEMENT_QUERY_BATCH,
                );
                assert_eq!(
                    measured_rows, expected_rows,
                    "multi-statement result cardinality"
                );
                measured_rows
            },
        );
    }
}

fn bench_simple_query(runner: &mut stress::CassieStressRunner, runtime: &tokio::runtime::Runtime) {
    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "pgwire_simple_query", dataset);
        let case =
            stress::StressCase::fixed_operations(4, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let ctx = runtime
            .block_on(workloads::pgwire_transport_context(
                &format!("tier4-pgwire-{dataset}"),
                rows,
            ))
            .expect("benchmark context");
        let sql =
            "SELECT id, title FROM bench_documents WHERE title = 'title-1' ORDER BY id ASC LIMIT 20";
        for _ in 0..SIMPLE_QUERY_WARMUP_BATCHES {
            let _ = pgwire_simple_query_batch(runtime, &ctx, sql, SIMPLE_QUERY_BATCH);
        }
        runner.fixed_timed_count(
            case.metadata("operation_unit", "result_row"),
            logical_operations(SIMPLE_QUERY_BATCH * SIMPLE_QUERY_ROWS),
            || pgwire_simple_query_batch(runtime, &ctx, sql, SIMPLE_QUERY_BATCH),
        );
    }
}

fn bench_prepared_query(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
) {
    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "pgwire_prepared_query", dataset);
        let case =
            stress::StressCase::fixed_operations(4, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let ctx = runtime
            .block_on(workloads::pgwire_prepared_context(
                &format!("tier4-pgwire-prepared-{dataset}"),
                rows,
            ))
            .expect("prepared pgwire benchmark context");
        runner.fixed_timed_count(
            case.metadata("operation_unit", "result_row"),
            logical_operations(PREPARED_QUERY_BATCH * PREPARED_QUERY_ROWS),
            || pgwire_prepared_query_batch(runtime, &ctx, PREPARED_QUERY_BATCH),
        );
    }
}

fn bench_legacy_rows(runner: &mut stress::CassieStressRunner, runtime: &tokio::runtime::Runtime) {
    let rows = [
        stress::StressCase::fixed_operations(4, "connection_churn", "10k"),
        stress::StressCase::fixed_operations(4, "connection_pooling", "10k"),
        stress::StressCase::fixed_operations(4, "large_result_set", "512_rows"),
        stress::StressCase::fixed_operations(4, "concurrent_connections", "8x10k")
            .parameter("client_count", "8"),
    ];
    if !rows.iter().any(|case| runner.is_enabled(case)) {
        return;
    }

    let ctx = runtime
        .block_on(workloads::pgwire_transport_context(
            "tier4-pgwire-legacy-10k",
            10_000,
        ))
        .expect("benchmark context");
    runner.fixed_timed_count(
        rows[0].clone().metadata("operation_unit", "result_row"),
        logical_operations(CONNECTION_CHURN_BATCH * SIMPLE_QUERY_ROWS),
        || pgwire_connection_churn_batch(runtime, &ctx, CONNECTION_CHURN_BATCH),
    );
    runner.fixed_operations(rows[1].clone(), || {
        runtime.block_on(workloads::pgwire_transport_simple_query(
            &ctx,
            "SELECT id FROM bench_documents WHERE score = 1 LIMIT 20",
        ))
    });
    runner.fixed_timed_count(
        rows[2].clone().metadata("operation_unit", "result_row"),
        logical_operations(LARGE_RESULT_ROWS),
        || {
            runtime.block_on(workloads::pgwire_transport_simple_query(
                &ctx,
                "SELECT id, title, body, score FROM bench_documents ORDER BY id LIMIT 512",
            ))
        },
    );
    runner.fixed_timed_count(
        rows[3]
            .clone()
            .metadata("operation_unit", "connection_query"),
        logical_operations(CONCURRENT_CLIENTS),
        || {
            runtime.block_on(workloads::pgwire_transport_concurrent_connections(
                &ctx,
                CONCURRENT_CLIENTS,
            ))
        },
    );
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

fn pgwire_multi_statement_query_batch(
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

fn pgwire_prepared_query_batch(
    runtime: &tokio::runtime::Runtime,
    ctx: &workloads::PgwirePreparedBenchContext,
    queries: usize,
) -> usize {
    let mut rows = 0usize;
    for _ in 0..queries {
        rows = rows.saturating_add(runtime.block_on(workloads::pgwire_prepared_query(ctx)));
    }
    std::hint::black_box(rows)
}

fn pgwire_connection_churn_batch(
    runtime: &tokio::runtime::Runtime,
    ctx: &workloads::PgwireTransportBenchContext,
    queries: usize,
) -> usize {
    let mut rows = 0usize;
    for _ in 0..queries {
        rows = rows
            .saturating_add(runtime.block_on(workloads::pgwire_transport_connection_churn(ctx)));
    }
    std::hint::black_box(rows)
}

fn logical_operations(operations: usize) -> u64 {
    u64::try_from(operations).expect("benchmark operation count should fit u64")
}
