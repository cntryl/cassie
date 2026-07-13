const BENCHMARK: &str = "tier4_integration_http";
const HTTP_BATCH_SIZE: usize = 16;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    workloads::configure_http_tls().expect("configure benchmark REST TLS identity");
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
        let http = runtime
            .block_on(workloads::http_transport_context(&context))
            .expect("http transport context");
        runner.fixed_timed_count(
            case.metadata("operation_unit", "document_create_get_workflow"),
            u64::try_from(HTTP_BATCH_SIZE).expect("HTTP batch size should fit u64"),
            || {
                runtime.block_on(workloads::http_transport_document_create_get_batch(
                    &http,
                    HTTP_BATCH_SIZE,
                ))
            },
        );
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

    if runner.is_enabled(&vector_case) {
        let vector_ctx = runtime
            .block_on(workloads::context_with_mock_tei_embeddings(
                "tier4-http-vector",
                10_000,
            ))
            .expect("vector benchmark context");
        let http = runtime
            .block_on(workloads::http_transport_context(&vector_ctx))
            .expect("http transport context");
        runner.fixed_timed_count(
            vector_case.metadata("operation_unit", "result_row"),
            10,
            || runtime.block_on(workloads::http_transport_vector_search(&http)),
        );
    }

    if needs_standard_context {
        let standard_context = runtime
            .block_on(workloads::unindexed_context("tier4-http", 10_000))
            .expect("benchmark context");
        let http = runtime
            .block_on(workloads::http_transport_context(&standard_context))
            .expect("http transport context");
        runner.fixed_timed_count(
            standard_rows[0]
                .clone()
                .metadata("operation_unit", "document_get_request"),
            512,
            || runtime.block_on(workloads::http_transport_large_result_set(&http)),
        );
        runner.fixed_batch(
            standard_rows[1]
                .clone()
                .metadata("operation_unit", "document_get_request"),
            8,
            || runtime.block_on(workloads::http_transport_concurrent_document_gets(&http, 8)),
        );
    }
}
