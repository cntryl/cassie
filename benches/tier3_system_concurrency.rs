use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_concurrency(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-concurrency", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier3_system_concurrency");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(8));

    group.bench_function(BenchmarkId::new("concurrent_queries", "8x10k"), |b| {
        b.iter(|| runtime.block_on(workloads::concurrent_queries(&ctx, 8)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_concurrency
}

criterion_main!(benches);
