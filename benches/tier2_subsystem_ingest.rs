use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_ingest(c: &mut Criterion) {
    const BENCHMARK: &str = "tier2_subsystem_ingest";

    let runtime = workloads::runtime();
    let ctx_10k = runtime
        .block_on(workloads::context("tier2-ingest", 10_000))
        .expect("benchmark context");
    let ctx_100k = runtime
        .block_on(workloads::replay_context("tier2-ingest-100k", 100_000))
        .expect("100k benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_ingest");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));
    let mut replay_nonce = 0usize;

    group.bench_function("projection_write_path", |b| {
        b.iter_custom(|iterations| {
            let mut elapsed = std::time::Duration::ZERO;
            for _ in 0..iterations {
                elapsed += runtime.block_on(workloads::timed_ingest_document_batch(&ctx_10k, 64));
            }
            elapsed
        });
    });
    group.bench_function("projection_duplicate_replay", |b| {
        b.iter(|| {
            replay_nonce = replay_nonce.wrapping_add(1);
            runtime.block_on(workloads::projection_duplicate_replay(
                &ctx_10k,
                replay_nonce,
            ))
        });
    });
    for (dataset, ctx) in [("10k", &ctx_10k), ("100k", &ctx_100k)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "projection_lag_catchup", dataset);
        group.bench_function(
            BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
            |b| {
                b.iter(|| {
                    replay_nonce = replay_nonce.wrapping_add(1);
                    runtime.block_on(workloads::projection_lag_catchup(ctx, replay_nonce))
                });
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2_write();
    targets = bench_ingest
}

criterion_main!(benches);
