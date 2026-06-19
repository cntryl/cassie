use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_pgwire(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier4-pgwire", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier4_integration_pgwire");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("pgwire_simple_query", "10k"), |b| {
        b.iter(|| {
            runtime.block_on(workloads::pgwire_simple_query(
                &ctx,
                "SELECT id, title FROM bench_documents WHERE title = 'title-1'",
            ))
        })
    });
    group.bench_function(
        BenchmarkId::new("prepared_statement_loop", "protocol"),
        |b| b.iter(workloads::pgwire_prepared_statement_protocol_loop),
    );
    group.bench_function(BenchmarkId::new("connection_churn", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::pgwire_connection_churn(&ctx)))
    });
    group.bench_function(BenchmarkId::new("connection_pooling", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::pgwire_connection_pooling(&ctx)))
    });
    group.bench_function(BenchmarkId::new("large_result_set", "512_rows"), |b| {
        b.iter(|| runtime.block_on(workloads::pgwire_large_result_query(&ctx)))
    });
    group.throughput(Throughput::Elements(8));
    group.bench_function(BenchmarkId::new("concurrent_connections", "8x10k"), |b| {
        b.iter(|| runtime.block_on(workloads::pgwire_concurrent_connections(&ctx, 8)))
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier4();
    targets = bench_pgwire
}

criterion_main!(benches);
