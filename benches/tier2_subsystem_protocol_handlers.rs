use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_protocol_handlers(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-protocol-handlers", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_protocol_handlers");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("postgres_wire_handler", "10k"), |b| {
        b.iter(|| {
            runtime.block_on(workloads::pgwire_simple_query(
                &ctx,
                "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20",
            ))
        });
    });
    group.bench_function(BenchmarkId::new("http_handler", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::http_document_get(&ctx)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_protocol_handlers
}

criterion_main!(benches);
