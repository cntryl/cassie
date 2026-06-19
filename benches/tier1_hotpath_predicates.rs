use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_predicates(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_predicates");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("field_lookup_by_field_id", |b| {
        b.iter(workloads::field_lookup_by_field_id)
    });
    group.bench_function("predicate_evaluation", |b| {
        b.iter(workloads::predicate_evaluation)
    });
    group.bench_function("query_parameter_binding", |b| {
        b.iter(workloads::parameter_binding)
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_predicates
}

criterion_main!(benches);
