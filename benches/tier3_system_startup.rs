use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_startup(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let warm_ctx = runtime
        .block_on(workloads::context("tier3-warm-start", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier3_system_startup");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("cold_start", "10k"), |b| {
        b.iter(|| runtime.block_on(workloads::empty_context("tier3-cold-start")));
    });
    group.bench_function(BenchmarkId::new("warm_start_query", "10k"), |b| {
        b.iter(|| {
            runtime.block_on(workloads::execute_sql(
                &warm_ctx,
                "SELECT count(*) FROM bench_documents",
            ))
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_startup
}

criterion_main!(benches);
