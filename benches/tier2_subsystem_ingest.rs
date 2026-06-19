use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_ingest(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-ingest", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_ingest");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("projection_write_path", |b| {
        b.iter(|| runtime.block_on(workloads::ingest_document(&ctx)))
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_ingest
}

criterion_main!(benches);
