use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_filter_projection(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_filter_projection");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("predicate_evaluation", |b| {
        b.iter(workloads::predicate_evaluation)
    });
    group.bench_function("batch_filter", |b| b.iter(workloads::batch_filter));
    group.bench_function("batch_projection", |b| b.iter(workloads::batch_projection));
    group.bench_function("value_comparison", |b| b.iter(workloads::value_comparison));
    group.bench_function("top_k_update", |b| b.iter(workloads::top_k_update));

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_filter_projection
}

criterion_main!(benches);
