use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

const BENCHMARK: &str = "tier4_integration_pgwire";

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn criterion_filters() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with("--"))
        .collect()
}

fn benchmark_enabled(filters: &[String], workload: &str, scale: &str) -> bool {
    if filters.is_empty() {
        return true;
    }
    let id = format!("{BENCHMARK}/{workload}/{scale}");
    filters
        .iter()
        .any(|filter| id.contains(filter) || workload.contains(filter) || scale == filter)
}

fn bench_pgwire(c: &mut Criterion) {
    let filters = criterion_filters();
    let runtime = workloads::runtime();

    let mut group = c.benchmark_group(BENCHMARK);
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        if !benchmark_enabled(&filters, "pgwire_simple_query", dataset) {
            continue;
        }
        let ctx = runtime
            .block_on(workloads::unindexed_context(
                &format!("tier4-pgwire-{dataset}"),
                rows,
            ))
            .expect("benchmark context");
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "pgwire_simple_query", dataset);
        group.bench_function(
            BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
            |b| {
                b.iter(|| {
                    runtime.block_on(workloads::pgwire_simple_query(
                        &ctx,
                        "SELECT id, title FROM bench_documents WHERE id = 'doc-1'",
                    ))
                });
            },
        );
    }

    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        if !benchmark_enabled(&filters, "pgwire_prepared_query", dataset) {
            continue;
        }
        let ctx = runtime
            .block_on(workloads::pgwire_prepared_context(
                &format!("tier4-pgwire-prepared-{dataset}"),
                rows,
            ))
            .expect("prepared pgwire benchmark context");
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "pgwire_prepared_query", dataset);
        group.bench_function(
            BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
            |b| b.iter(|| runtime.block_on(workloads::pgwire_prepared_query(&ctx))),
        );
    }

    if benchmark_enabled(&filters, "prepared_statement_loop", "protocol") {
        group.bench_function(
            BenchmarkId::new("prepared_statement_loop", "protocol"),
            |b| b.iter(workloads::pgwire_prepared_statement_protocol_loop),
        );
    }

    let needs_legacy_ctx = [
        ("connection_churn", "10k"),
        ("connection_pooling", "10k"),
        ("large_result_set", "512_rows"),
        ("concurrent_connections", "8x10k"),
    ]
    .into_iter()
    .any(|(workload, scale)| benchmark_enabled(&filters, workload, scale));
    if needs_legacy_ctx {
        let ctx_10k = runtime
            .block_on(workloads::unindexed_context(
                "tier4-pgwire-legacy-10k",
                10_000,
            ))
            .expect("benchmark context");
        if benchmark_enabled(&filters, "connection_churn", "10k") {
            group.bench_function(BenchmarkId::new("connection_churn", "10k"), |b| {
                b.iter(|| runtime.block_on(workloads::pgwire_connection_churn(&ctx_10k)));
            });
        }
        if benchmark_enabled(&filters, "connection_pooling", "10k") {
            group.bench_function(BenchmarkId::new("connection_pooling", "10k"), |b| {
                b.iter(|| runtime.block_on(workloads::pgwire_connection_pooling(&ctx_10k)));
            });
        }
        if benchmark_enabled(&filters, "large_result_set", "512_rows") {
            group.bench_function(BenchmarkId::new("large_result_set", "512_rows"), |b| {
                b.iter(|| runtime.block_on(workloads::pgwire_large_result_query(&ctx_10k)));
            });
        }
        if benchmark_enabled(&filters, "concurrent_connections", "8x10k") {
            group.throughput(Throughput::Elements(8));
            group.bench_function(BenchmarkId::new("concurrent_connections", "8x10k"), |b| {
                b.iter(|| runtime.block_on(workloads::pgwire_concurrent_connections(&ctx_10k, 8)));
            });
        }
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier4();
    targets = bench_pgwire
}

criterion_main!(benches);
