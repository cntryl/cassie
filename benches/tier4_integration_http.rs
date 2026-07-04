const BENCHMARK: &str = "tier4_integration_http";
const HTTP_BATCH_OPERATIONS: u64 = 64;
const HTTP_BATCH_SIZE: usize = 64;

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    bench_document_create_get(&mut runner, &runtime);
    bench_legacy_rows(&mut runner, &runtime);

    runner.finish();
}

fn bench_document_create_get(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
) {
    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let benchmark = performance_benchmarks::expect_benchmark(
            BENCHMARK,
            "http_document_create_get",
            dataset,
        );
        let case =
            stress::StressCase::fixed_operations(4, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let context = runtime
            .block_on(workloads::unindexed_context(
                &format!("tier4-http-{dataset}"),
                rows,
            ))
            .expect("benchmark context");
        runner.external_timed_batch(case, HTTP_BATCH_OPERATIONS, || {
            runtime.block_on(workloads::timed_http_document_create_get_batch(
                &context,
                HTTP_BATCH_SIZE,
            ))
        });
    }
}

fn bench_legacy_rows(runner: &mut stress::CassieStressRunner, runtime: &tokio::runtime::Runtime) {
    let standard_rows = [
        stress::StressCase::fixed_operations(4, "http_large_result_set", "512_rows"),
        stress::StressCase::fixed_operations(4, "http_concurrent_requests", "8x10k")
            .parameter("client_count", "8"),
    ];
    let needs_standard_context = standard_rows.iter().any(|case| runner.is_enabled(case));
    let vector_case = stress::StressCase::fixed_operations(4, "http_vector_search", "10k");
    let serialization_case =
        stress::StressCase::fixed_operations(4, "json_serialization_overhead", "512_rows");

    if runner.is_enabled(&vector_case) {
        let vector_ctx = runtime
            .block_on(workloads::context_with_mock_tei_embeddings(
                "tier4-http-vector",
                10_000,
            ))
            .expect("vector benchmark context");
        runner.fixed_operations(vector_case, || {
            runtime.block_on(workloads::http_vector_search(&vector_ctx))
        });
    }

    if needs_standard_context {
        let standard_context = runtime
            .block_on(workloads::unindexed_context("tier4-http", 10_000))
            .expect("benchmark context");
        runner.fixed_operations(standard_rows[0].clone(), || {
            runtime.block_on(workloads::http_large_result_json(&standard_context))
        });
        runner.fixed_operations(standard_rows[1].clone(), || {
            runtime.block_on(workloads::http_concurrent_document_gets(
                &standard_context,
                8,
            ))
        });
    }

    runner.fixed_operations(serialization_case, workloads::json_serialization_overhead);
}
