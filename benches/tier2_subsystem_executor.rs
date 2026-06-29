use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};
use std::hint::black_box;

const BENCHMARK: &str = "tier2_subsystem_executor";

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn criterion_filters() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with("--"))
        .collect()
}

fn benchmark_enabled(filters: &[String], workload: &str, scale: &str) -> bool {
    if filters.is_empty() {
        return true;
    }
    let id = format!("{BENCHMARK}/{workload}/{scale}");
    filters
        .iter()
        .any(|filter| id.contains(filter) || workload.contains(filter) || scale == filter)
}

fn bench_executor(c: &mut Criterion) {
    std::env::set_var("CASSIE_PARALLEL_AGGREGATION_WORKERS", "4");
    let filters = criterion_filters();
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-executor", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group(BENCHMARK);
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
        if !benchmark_enabled(&filters, name, "10k") {
            continue;
        }
        group.bench_function(BenchmarkId::new(name, "10k"), |b| {
            b.iter(|| black_box(runtime.block_on(workloads::execute_sql(&ctx, sql))));
        });
    }

    for dataset_rows in [10_000, 100_000] {
        let scale = if dataset_rows == 10_000 {
            "10k"
        } else {
            "100k"
        };
        if benchmark_enabled(&filters, "column_batch_covered_projection", scale) {
            let column_ctx = runtime
                .block_on(workloads::column_batch_context(
                    &format!("tier2-executor-column-{scale}"),
                    dataset_rows,
                ))
                .expect("column-batch benchmark context");
            let column_sql =
                "SELECT title, body FROM bench_documents WHERE status = 'approved' LIMIT 50";
            black_box(runtime.block_on(workloads::execute_sql(&column_ctx, column_sql)));
            let benchmark = performance_benchmarks::expect_benchmark(
                BENCHMARK,
                "column_batch_covered_projection",
                scale,
            );
            group.bench_function(
                BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
                |b| {
                    b.iter(|| {
                        black_box(runtime.block_on(workloads::execute_sql(&column_ctx, column_sql)))
                    });
                },
            );
        }

        if benchmark_enabled(&filters, "vectorized_join_equi", scale)
            || benchmark_enabled(&filters, "vectorized_left_join_limited", scale)
        {
            let join_ctx = runtime
                .block_on(workloads::vectorized_join_context(
                    &format!("tier2-executor-vectorized-join-{scale}"),
                    dataset_rows,
                ))
                .expect("vectorized join benchmark context");
            if benchmark_enabled(&filters, "vectorized_join_equi", scale) {
                let join_sql = "SELECT bench_join_users.name, bench_join_orders.total \
                                FROM bench_join_users JOIN bench_join_orders \
                                ON bench_join_users.user_key = bench_join_orders.order_user_key \
                                LIMIT 50";
                black_box(runtime.block_on(workloads::execute_sql(&join_ctx, join_sql)));
                let benchmark = performance_benchmarks::expect_benchmark(
                    BENCHMARK,
                    "vectorized_join_equi",
                    scale,
                );
                group.bench_function(
                    BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
                    |b| {
                        b.iter(|| {
                            black_box(runtime.block_on(workloads::execute_sql(&join_ctx, join_sql)))
                        });
                    },
                );
            }
            if benchmark_enabled(&filters, "vectorized_left_join_limited", scale) {
                let left_join_sql = "SELECT bench_join_users.name, bench_join_orders.total \
                                     FROM bench_join_users LEFT JOIN bench_join_orders \
                                     ON bench_join_users.user_key = bench_join_orders.order_user_key \
                                     LIMIT 50";
                black_box(runtime.block_on(workloads::execute_sql(&join_ctx, left_join_sql)));
                let benchmark = performance_benchmarks::expect_benchmark(
                    BENCHMARK,
                    "vectorized_left_join_limited",
                    scale,
                );
                group.bench_function(
                    BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
                    |b| {
                        b.iter(|| {
                            black_box(
                                runtime.block_on(workloads::execute_sql(&join_ctx, left_join_sql)),
                            )
                        });
                    },
                );
            }
        }

        if benchmark_enabled(&filters, "vectorized_indexed_inner_join", scale) {
            let indexed_join_ctx = runtime
                .block_on(workloads::vectorized_indexed_join_context(
                    &format!("tier2-executor-vectorized-indexed-join-{scale}"),
                    dataset_rows,
                ))
                .expect("vectorized indexed join benchmark context");
            let indexed_join_sql = "SELECT bench_join_users.name, bench_join_orders.total \
                                    FROM bench_join_users JOIN bench_join_orders \
                                    ON bench_join_users.user_key = bench_join_orders.order_user_key \
                                    LIMIT 50";
            black_box(
                runtime.block_on(workloads::execute_sql(&indexed_join_ctx, indexed_join_sql)),
            );
            let benchmark = performance_benchmarks::expect_benchmark(
                BENCHMARK,
                "vectorized_indexed_inner_join",
                scale,
            );
            group.bench_function(
                BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
                |b| {
                    b.iter(|| {
                        black_box(
                            runtime.block_on(workloads::execute_sql(
                                &indexed_join_ctx,
                                indexed_join_sql,
                            )),
                        )
                    });
                },
            );
        }

        if benchmark_enabled(&filters, "vectorized_streaming_inner_join", scale) {
            let streaming_join_ctx = runtime
                .block_on(workloads::vectorized_sparse_join_context(
                    &format!("tier2-executor-vectorized-streaming-join-{scale}"),
                    dataset_rows,
                ))
                .expect("vectorized streaming join benchmark context");
            let streaming_join_sql = "SELECT bench_join_users.name, bench_join_orders.total \
                                      FROM bench_join_users JOIN bench_join_orders \
                                      ON bench_join_users.user_key = bench_join_orders.order_user_key \
                                      LIMIT 50";
            black_box(runtime.block_on(workloads::execute_sql(
                &streaming_join_ctx,
                streaming_join_sql,
            )));
            let benchmark = performance_benchmarks::expect_benchmark(
                BENCHMARK,
                "vectorized_streaming_inner_join",
                scale,
            );
            group.bench_function(
                BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
                |b| {
                    b.iter(|| {
                        black_box(runtime.block_on(workloads::execute_sql(
                            &streaming_join_ctx,
                            streaming_join_sql,
                        )))
                    });
                },
            );
        }

        if benchmark_enabled(&filters, "vectorized_dense_streaming_inner_join", scale) {
            let dense_join_ctx = runtime
                .block_on(workloads::vectorized_dense_join_context(
                    &format!("tier2-executor-vectorized-dense-streaming-join-{scale}"),
                    dataset_rows,
                ))
                .expect("vectorized dense streaming join benchmark context");
            let dense_join_sql = "SELECT bench_join_users.name, bench_join_orders.total \
                                  FROM bench_join_users JOIN bench_join_orders \
                                  ON bench_join_users.user_key = bench_join_orders.order_user_key \
                                  LIMIT 2";
            black_box(runtime.block_on(workloads::execute_sql(&dense_join_ctx, dense_join_sql)));
            let benchmark = performance_benchmarks::expect_benchmark(
                BENCHMARK,
                "vectorized_dense_streaming_inner_join",
                scale,
            );
            group.bench_function(
                BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
                |b| {
                    b.iter(|| {
                        black_box(
                            runtime
                                .block_on(workloads::execute_sql(&dense_join_ctx, dense_join_sql)),
                        )
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_executor
}

criterion_main!(benches);
