use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_bm25(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_bm25");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("bm25_scoring", |b| b.iter(workloads::bm25_score));

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_bm25
}

criterion_main!(benches);
