use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_row_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_row_codec");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("row_encode_decode", |b| {
        b.iter(workloads::row_encode_decode)
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_row_codec
}

criterion_main!(benches);
