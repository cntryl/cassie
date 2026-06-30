use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_query_breakdown(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-query-breakdown", 10_000))
        .expect("benchmark context");

    let breakdown = (0..12)
        .map(|_| runtime.block_on(workloads::simple_10k_query_breakdown(&ctx)))
        .min_by_key(|breakdown| breakdown.total)
        .expect("query breakdown sample");
    println!(
        "{}",
        serde_json::to_string_pretty(&breakdown).expect("serialize query breakdown")
    );

    let mut group = c.benchmark_group("tier3_system_query_breakdown");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("simple_10k", "breakdown"), |b| {
        b.iter(|| runtime.block_on(workloads::simple_10k_query_breakdown(&ctx)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_query_breakdown
}

criterion_main!(benches);
