use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};
use std::hint::black_box;

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_executor(c: &mut Criterion) {
    std::env::set_var("CASSIE_PARALLEL_AGGREGATION_WORKERS", "4");
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
            "scalar_index_seek_executor",
            "SELECT id FROM bench_documents WHERE title = 'title-1' ORDER BY id ASC LIMIT 25",
        ),
        (
            "scalar_range_scan_executor",
            "SELECT id FROM bench_documents WHERE title >= 'title-04' AND title < 'title-09' ORDER BY title ASC LIMIT 25",
        ),
        (
            "scalar_ordered_bounded_executor",
            "SELECT id FROM bench_documents ORDER BY title ASC LIMIT 25",
        ),
        (
            "large_ordered_scan_executor",
            "SELECT id FROM bench_documents ORDER BY title ASC LIMIT 25 OFFSET 9000",
        ),
        (
            "row_id_storage_top_k_executor",
            "SELECT id FROM bench_documents ORDER BY id ASC LIMIT 25",
        ),
        (
            "row_id_keyset_executor",
            "SELECT id FROM bench_documents WHERE id > 'doc-09000' ORDER BY id ASC LIMIT 25",
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
            "parallel_aggregation_grouped_executor",
            "SELECT status, COUNT(*) AS total, SUM(score) AS sum_score, AVG(score) AS avg_score FROM bench_documents GROUP BY status ORDER BY status",
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
            b.iter(|| black_box(runtime.block_on(workloads::execute_sql(&ctx, sql))))
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
