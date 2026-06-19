use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_vector_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_vector_distance");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("cosine_distance", |b| b.iter(workloads::cosine_distance));
    group.bench_function("dot_product", |b| b.iter(workloads::dot_product));
    group.bench_function("l2_distance", |b| b.iter(workloads::l2_distance));

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_vector_distance
}

criterion_main!(benches);
