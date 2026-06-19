use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier2_subsystem_parser");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("sql_lexer", |b| b.iter(workloads::sql_lexing));
    group.bench_function("sql_parser", |b| b.iter(workloads::sql_parsing));

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_parser
}

criterion_main!(benches);
