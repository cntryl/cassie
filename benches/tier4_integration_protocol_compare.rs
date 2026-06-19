use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_protocol_compare(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier4-protocol-compare", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier4_integration_protocol_compare");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("direct_query_baseline", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::protocol_comparison_sql(&ctx)))
    });
    group.bench_function(BenchmarkId::new("postgres_wire_query", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::protocol_comparison_pgwire(&ctx)))
    });
    group.bench_function(BenchmarkId::new("http_json_query", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::protocol_comparison_http(&ctx)))
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier4();
    targets = bench_protocol_compare
}

criterion_main!(benches);
