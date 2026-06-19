use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_keys(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_keys");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("key_encode_decode", |b| {
        b.iter(workloads::key_encode_decode)
    });
    group.bench_function("field_lookup", |b| b.iter(workloads::field_lookup));

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_keys
}

criterion_main!(benches);
