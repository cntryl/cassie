use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_pgwire_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_pgwire_json");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("query_parameter_binding", |b| {
        b.iter(workloads::parameter_binding)
    });
    group.bench_function("row_to_pgwire_encoding", |b| {
        b.iter(workloads::row_to_pgwire_encoding)
    });
    group.bench_function("row_to_json_encoding", |b| {
        b.iter(workloads::row_to_json_encoding)
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_pgwire_json
}

criterion_main!(benches);
