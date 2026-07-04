const BENCHMARK: &str = "tier4_integration_pgwire";

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    bench_simple_query(&mut runner, &runtime);
    bench_prepared_query(&mut runner, &runtime);
    runner.fixed_operations(
        stress::StressCase::fixed_operations(4, "prepared_statement_loop", "protocol"),
        workloads::pgwire_prepared_statement_protocol_loop,
    );
    bench_legacy_rows(&mut runner, &runtime);

    runner.finish();
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
            .block_on(workloads::unindexed_context(
                &format!("tier4-pgwire-{dataset}"),
                rows,
            ))
            .expect("benchmark context");
        runner.fixed_operations(case, || {
            runtime.block_on(workloads::pgwire_simple_query(
                &ctx,
                "SELECT id, title FROM bench_documents WHERE id = 'doc-1'",
            ))
        });
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
        runner.fixed_operations(case, || {
            runtime.block_on(workloads::pgwire_prepared_query(&ctx))
        });
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
        .block_on(workloads::unindexed_context(
            "tier4-pgwire-legacy-10k",
            10_000,
        ))
        .expect("benchmark context");
    runner.fixed_operations(rows[0].clone(), || {
        runtime.block_on(workloads::pgwire_connection_churn(&ctx))
    });
    runner.fixed_operations(rows[1].clone(), || {
        runtime.block_on(workloads::pgwire_connection_pooling(&ctx))
    });
    runner.fixed_operations(rows[2].clone(), || {
        runtime.block_on(workloads::pgwire_large_result_query(&ctx))
    });
    runner.fixed_operations(rows[3].clone(), || {
        runtime.block_on(workloads::pgwire_concurrent_connections(&ctx, 8))
    });
}
