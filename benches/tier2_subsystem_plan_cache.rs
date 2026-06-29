use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_plan_cache(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-plan-cache", 1_024))
        .expect("benchmark context");
    runtime.block_on(workloads::plan_cache_hit(&ctx));
    let mut miss_nonce = 0usize;

    let mut group = c.benchmark_group("tier2_subsystem_plan_cache");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("plan_cache_hit", |b| {
        b.iter(|| runtime.block_on(workloads::plan_cache_hit(&ctx)));
    });
    group.bench_function("plan_cache_miss", |b| {
        b.iter(|| {
            miss_nonce = miss_nonce.wrapping_add(1);
            runtime.block_on(workloads::plan_cache_miss(&ctx, miss_nonce))
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_plan_cache
}

criterion_main!(benches);
