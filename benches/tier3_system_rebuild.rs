use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_rebuild(c: &mut Criterion) {
    const BENCHMARK: &str = "tier3_system_rebuild";

    let runtime = workloads::runtime();
    let ctx_10k = runtime
        .block_on(workloads::context("tier3-rebuild", 10_000))
        .expect("benchmark context");
    let ctx_100k = runtime
        .block_on(workloads::unindexed_context("tier3-rebuild-100k", 100_000))
        .expect("100k benchmark context");
    let time_series_ctx_10k = runtime
        .block_on(workloads::time_series_context("tier3-rebuild-ts", 10_000))
        .expect("time-series benchmark context");
    let time_series_ctx_100k = runtime
        .block_on(workloads::time_series_context(
            "tier3-rebuild-ts-100k",
            100_000,
        ))
        .expect("100k time-series benchmark context");
    let mut index_nonce = 0usize;
    let mut retention_nonce = 0usize;
    let mut rollup_nonce = 0usize;

    let mut group = c.benchmark_group("tier3_system_rebuild");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("projection_rebuild_query", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::projection_rebuild_query(&ctx_10k)))
    });
    performance_benchmarks::expect_benchmark(BENCHMARK, "projection_refresh", "10k");
    group.bench_function(BenchmarkId::new("projection_refresh", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::projection_refresh_workflow(&ctx_10k)))
    });
    performance_benchmarks::expect_benchmark(BENCHMARK, "projection_verify", "10k");
    group.bench_function(BenchmarkId::new("projection_verify", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::projection_rebuild_verification(&ctx_10k)))
    });
    let refresh_100k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "projection_refresh", "100k");
    group.bench_function(
        BenchmarkId::new(refresh_100k.workload, refresh_100k.fixture_scale),
        |b| b.iter(|| runtime.block_on(workloads::projection_refresh_workflow(&ctx_100k))),
    );
    let verify_100k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "projection_verify", "100k");
    group.bench_function(
        BenchmarkId::new(verify_100k.workload, verify_100k.fixture_scale),
        |b| b.iter(|| runtime.block_on(workloads::projection_rebuild_verification(&ctx_100k))),
    );
    group.bench_function(BenchmarkId::new("projection_swap", "10k"), |b| {
        b.iter(|| {
            index_nonce = index_nonce.wrapping_add(1);
            runtime.block_on(workloads::projection_version_swap(&ctx_10k, index_nonce))
        })
    });
    group.bench_function(BenchmarkId::new("index_rebuild_ddl", "10k"), |b| {
        b.iter(|| {
            index_nonce = index_nonce.wrapping_add(1);
            runtime.block_on(workloads::index_rebuild_ddl(&ctx_10k, index_nonce))
        })
    });
    let retention_10k = performance_benchmarks::expect_benchmark(
        BENCHMARK,
        "time_series_retention_enforcement",
        "10k",
    );
    group.bench_function(
        BenchmarkId::new(retention_10k.workload, retention_10k.fixture_scale),
        |b| {
            b.iter(|| {
                retention_nonce = retention_nonce.wrapping_add(1);
                runtime.block_on(workloads::time_series_retention_enforcement(
                    &time_series_ctx_10k,
                    retention_nonce,
                ))
            })
        },
    );
    let retention_100k = performance_benchmarks::expect_benchmark(
        BENCHMARK,
        "time_series_retention_enforcement",
        "100k",
    );
    group.bench_function(
        BenchmarkId::new(retention_100k.workload, retention_100k.fixture_scale),
        |b| {
            b.iter(|| {
                retention_nonce = retention_nonce.wrapping_add(1);
                runtime.block_on(workloads::time_series_retention_enforcement(
                    &time_series_ctx_100k,
                    retention_nonce,
                ))
            })
        },
    );
    let rollup_10k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_rollup_refresh", "10k");
    group.bench_function(
        BenchmarkId::new(rollup_10k.workload, rollup_10k.fixture_scale),
        |b| {
            b.iter(|| {
                rollup_nonce = rollup_nonce.wrapping_add(1);
                runtime.block_on(workloads::time_series_rollup_refresh(
                    &time_series_ctx_10k,
                    rollup_nonce,
                ))
            })
        },
    );
    let rollup_100k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_rollup_refresh", "100k");
    group.bench_function(
        BenchmarkId::new(rollup_100k.workload, rollup_100k.fixture_scale),
        |b| {
            b.iter(|| {
                rollup_nonce = rollup_nonce.wrapping_add(1);
                runtime.block_on(workloads::time_series_rollup_refresh(
                    &time_series_ctx_100k,
                    rollup_nonce,
                ))
            })
        },
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_rebuild
}

criterion_main!(benches);
