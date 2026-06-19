use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_rebuild(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-rebuild", 10_000))
        .expect("benchmark context");
    let mut index_nonce = 0usize;

    let mut group = c.benchmark_group("tier3_system_rebuild");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("projection_rebuild_query", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::projection_rebuild_query(&ctx)))
    });
    group.bench_function(BenchmarkId::new("index_rebuild_ddl", "10k"), |b| {
        b.iter(|| {
            index_nonce = index_nonce.wrapping_add(1);
            runtime.block_on(workloads::index_rebuild_ddl(&ctx, index_nonce))
        })
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_rebuild
}

criterion_main!(benches);
