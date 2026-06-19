use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_http(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier4-http", 10_000))
        .expect("benchmark context");
    let vector_ctx = runtime
        .block_on(workloads::context_with_mock_tei_embeddings(
            "tier4-http-vector",
            10_000,
        ))
        .expect("vector benchmark context");

    let mut group = c.benchmark_group("tier4_integration_http");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("http_document_create_get", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::http_document_create_get(&ctx)))
    });
    group.bench_function(BenchmarkId::new("http_vector_search", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::http_vector_search(&vector_ctx)))
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier4();
    targets = bench_http
}

criterion_main!(benches);
