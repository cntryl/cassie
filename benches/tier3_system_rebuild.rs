use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_projection_rebuild_10k(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    runtime: &tokio::runtime::Runtime,
    index_nonce: &mut usize,
) {
    let context = runtime
        .block_on(workloads::context("tier3-rebuild", 10_000))
        .expect("benchmark context");
    group.bench_function(BenchmarkId::new("projection_rebuild_query", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::projection_rebuild_query(&context)));
    });
    performance_benchmarks::expect_benchmark("tier3_system_rebuild", "projection_refresh", "10k");
    group.bench_function(BenchmarkId::new("projection_refresh", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::projection_refresh_workflow(&context)));
    });
    performance_benchmarks::expect_benchmark("tier3_system_rebuild", "projection_verify", "10k");
    group.bench_function(BenchmarkId::new("projection_verify", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::projection_rebuild_verification(&context)));
    });
    group.bench_function(BenchmarkId::new("projection_swap", "10k"), |b| {
        b.iter(|| {
            *index_nonce = index_nonce.wrapping_add(1);
            runtime.block_on(workloads::projection_version_swap(&context, *index_nonce))
        });
    });
    group.bench_function(BenchmarkId::new("index_rebuild_ddl", "10k"), |b| {
        b.iter(|| {
            *index_nonce = index_nonce.wrapping_add(1);
            runtime.block_on(workloads::index_rebuild_ddl(&context, *index_nonce))
        });
    });
}

fn bench_projection_rebuild_100k(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    runtime: &tokio::runtime::Runtime,
) {
    let context = runtime
        .block_on(workloads::unindexed_context("tier3-rebuild-100k", 100_000))
        .expect("100k benchmark context");
    let refresh = performance_benchmarks::expect_benchmark(
        "tier3_system_rebuild",
        "projection_refresh",
        "100k",
    );
    group.bench_function(
        BenchmarkId::new(refresh.workload, refresh.fixture_scale),
        |b| b.iter(|| runtime.block_on(workloads::projection_refresh_workflow(&context))),
    );
    let verify = performance_benchmarks::expect_benchmark(
        "tier3_system_rebuild",
        "projection_verify",
        "100k",
    );
    group.bench_function(
        BenchmarkId::new(verify.workload, verify.fixture_scale),
        |b| b.iter(|| runtime.block_on(workloads::projection_rebuild_verification(&context))),
    );
}

fn bench_time_series_rebuild(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    runtime: &tokio::runtime::Runtime,
    rows: usize,
    label: &str,
    scale: &str,
    retention_nonce: &mut usize,
    rollup_nonce: &mut usize,
) {
    let context = runtime
        .block_on(workloads::time_series_context(label, rows))
        .expect("time-series benchmark context");
    let retention = performance_benchmarks::expect_benchmark(
        "tier3_system_rebuild",
        "time_series_retention_enforcement",
        scale,
    );
    group.bench_function(
        BenchmarkId::new(retention.workload, retention.fixture_scale),
        |b| {
            b.iter(|| {
                *retention_nonce = retention_nonce.wrapping_add(1);
                runtime.block_on(workloads::time_series_retention_enforcement(
                    &context,
                    *retention_nonce,
                ))
            });
        },
    );
    let rollup = performance_benchmarks::expect_benchmark(
        "tier3_system_rebuild",
        "time_series_rollup_refresh",
        scale,
    );
    group.bench_function(
        BenchmarkId::new(rollup.workload, rollup.fixture_scale),
        |b| {
            b.iter(|| {
                *rollup_nonce = rollup_nonce.wrapping_add(1);
                runtime.block_on(workloads::time_series_rollup_refresh(
                    &context,
                    *rollup_nonce,
                ))
            });
        },
    );
}

fn bench_rebuild(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let mut index_nonce = 0usize;
    let mut retention_nonce = 0usize;
    let mut rollup_nonce = 0usize;

    let mut group = c.benchmark_group("tier3_system_rebuild");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    bench_projection_rebuild_10k(&mut group, &runtime, &mut index_nonce);
    bench_projection_rebuild_100k(&mut group, &runtime);
    bench_time_series_rebuild(
        &mut group,
        &runtime,
        10_000,
        "tier3-rebuild-ts",
        "10k",
        &mut retention_nonce,
        &mut rollup_nonce,
    );
    bench_time_series_rebuild(
        &mut group,
        &runtime,
        100_000,
        "tier3-rebuild-ts-100k",
        "100k",
        &mut retention_nonce,
        &mut rollup_nonce,
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_rebuild
}

criterion_main!(benches);
