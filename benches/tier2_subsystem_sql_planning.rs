use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_sql_planning(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-sql-planning", 128))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_sql_planning");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("sql_parsing", |b| b.iter(workloads::sql_parsing));
    group.bench_function("sql_binding", |b| {
        b.iter(|| runtime.block_on(workloads::sql_binding(&ctx)));
    });
    group.bench_function("logical_planning", |b| {
        b.iter(|| runtime.block_on(workloads::logical_planning(&ctx)));
    });
    group.bench_function("physical_planning", |b| {
        b.iter(|| runtime.block_on(workloads::physical_planning(&ctx)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_sql_planning
}

criterion_main!(benches);
