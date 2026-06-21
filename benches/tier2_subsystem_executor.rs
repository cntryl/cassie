use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_executor(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-executor", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_executor");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    let cases = [
        (
            "simple_scan_executor",
            "SELECT id, title FROM bench_documents WHERE title = 'title-1'",
        ),
        (
            "large_ordered_scan_executor",
            "SELECT id FROM bench_documents ORDER BY title ASC LIMIT 25 OFFSET 9000",
        ),
        (
            "indexed_filter_executor",
            "SELECT id FROM bench_documents WHERE score = 1",
        ),
        (
            "fulltext_search_executor",
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha')",
        ),
        (
            "parallel_scoring_fulltext_executor",
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 25",
        ),
        (
            "vector_bruteforce_executor",
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 10",
        ),
        (
            "hybrid_executor",
            "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 10",
        ),
    ];

    for (name, sql) in cases {
        group.bench_function(BenchmarkId::new(name, "10k"), |b| {
            b.iter(|| runtime.block_on(workloads::execute_sql(&ctx, sql)))
        });
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_executor
}

criterion_main!(benches);
