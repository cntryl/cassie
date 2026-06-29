use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_mixed_load(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-mixed-load", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier3_system_mixed_load");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("mixed_ingest_query", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::mixed_ingest_query(&ctx)));
    });
    group.bench_function(BenchmarkId::new("large_result_set", "512_rows"), |b| {
        b.iter(|| runtime.block_on(workloads::large_result_set_query(&ctx)));
    });
    group.bench_function(
        BenchmarkId::new("ten_million_row_query_shape", "scaled"),
        |b| b.iter(|| runtime.block_on(workloads::ten_million_row_query_shape(&ctx))),
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_mixed_load
}

criterion_main!(benches);
