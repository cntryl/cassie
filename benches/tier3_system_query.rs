use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_query(c: &mut Criterion) {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-query", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group("tier3_system_query");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    let cases = [
        (
            "simple_sql_query",
            "10k",
            "SELECT id, title FROM bench_documents WHERE title = 'title-1'",
        ),
        (
            "indexed_filter_query",
            "1m",
            "SELECT id FROM bench_documents WHERE score = 1",
        ),
        (
            "range_query",
            "1m",
            "SELECT id FROM bench_documents WHERE score >= 10 LIMIT 100",
        ),
        (
            "sort_limit_query",
            "1m",
            "SELECT id FROM bench_documents ORDER BY score DESC LIMIT 50",
        ),
        (
            "fulltext_search_query",
            "1m",
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
        ),
        (
            "vector_search_query",
            "1m",
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20",
        ),
        (
            "hybrid_search_query",
            "1m",
            "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 20",
        ),
    ];

    for (name, dataset, sql) in cases {
        group.bench_function(BenchmarkId::new(name, dataset), |b| {
            b.iter(|| runtime.block_on(workloads::execute_sql(&ctx, sql)))
        });
    }

    group.bench_function(BenchmarkId::new("mixed_ingest_query_load", "1m"), |b| {
        b.iter(|| runtime.block_on(workloads::ingest_document(&ctx)))
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_query
}

criterion_main!(benches);
