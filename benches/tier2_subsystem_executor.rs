use std::hint::black_box;

use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, BenchmarkId, Criterion,
    SamplingMode, Throughput,
};

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
    let context = runtime
        .block_on(workloads::context("tier2-executor", 10_000))
        .expect("benchmark context");

    let mut group = c.benchmark_group(BENCHMARK);
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    bench_fixed_executor_cases(&mut group, &runtime, &context, &filters);
    for dataset_rows in [10_000, 100_000] {
        bench_scaled_executor_cases(&mut group, &runtime, &filters, dataset_rows);
    }

    group.finish();
}

fn bench_fixed_executor_cases(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    filters: &[String],
) {
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
        if benchmark_enabled(filters, name, "10k") {
            group.bench_function(BenchmarkId::new(name, "10k"), |b| {
                b.iter(|| black_box(runtime.block_on(workloads::execute_sql(context, sql))));
            });
        }
    }
}

fn bench_scaled_executor_cases(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    filters: &[String],
    dataset_rows: usize,
) {
    let scale = scale_label(dataset_rows);
    bench_column_batch_case(group, runtime, filters, dataset_rows, scale);
    bench_join_pair_cases(group, runtime, filters, dataset_rows, scale);
    bench_indexed_join_case(group, runtime, filters, dataset_rows, scale);
    bench_streaming_join_case(group, runtime, filters, dataset_rows, scale);
    bench_dense_streaming_join_case(group, runtime, filters, dataset_rows, scale);
}

fn scale_label(dataset_rows: usize) -> &'static str {
    if dataset_rows == 10_000 {
        "10k"
    } else {
        "100k"
    }
}

fn bench_column_batch_case(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    filters: &[String],
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "column_batch_covered_projection";
    if !benchmark_enabled(filters, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::column_batch_context(
            &format!("tier2-executor-column-{scale}"),
            dataset_rows,
        ))
        .expect("column-batch benchmark context");
    let sql = "SELECT title, body FROM bench_documents WHERE status = 'approved' LIMIT 50";
    bench_expected_sql_case(group, runtime, &context, workload, scale, sql);
}

fn bench_join_pair_cases(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    filters: &[String],
    dataset_rows: usize,
    scale: &str,
) {
    if !benchmark_enabled(filters, "vectorized_join_equi", scale)
        && !benchmark_enabled(filters, "vectorized_left_join_limited", scale)
    {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_join_context(
            &format!("tier2-executor-vectorized-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized join benchmark context");
    bench_optional_join_case(
        group,
        runtime,
        filters,
        &context,
        "vectorized_join_equi",
        scale,
        join_sql(50),
    );
    bench_optional_join_case(
        group,
        runtime,
        filters,
        &context,
        "vectorized_left_join_limited",
        scale,
        "SELECT bench_join_users.name, bench_join_orders.total \
         FROM bench_join_users LEFT JOIN bench_join_orders \
         ON bench_join_users.user_key = bench_join_orders.order_user_key \
         LIMIT 50",
    );
}

fn bench_indexed_join_case(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    filters: &[String],
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_indexed_inner_join";
    if !benchmark_enabled(filters, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_indexed_join_context(
            &format!("tier2-executor-vectorized-indexed-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized indexed join benchmark context");
    bench_expected_sql_case(group, runtime, &context, workload, scale, join_sql(50));
}

fn bench_streaming_join_case(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    filters: &[String],
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_streaming_inner_join";
    if !benchmark_enabled(filters, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_sparse_join_context(
            &format!("tier2-executor-vectorized-streaming-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized streaming join benchmark context");
    bench_expected_sql_case(group, runtime, &context, workload, scale, join_sql(50));
}

fn bench_dense_streaming_join_case(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    filters: &[String],
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_dense_streaming_inner_join";
    if !benchmark_enabled(filters, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_dense_join_context(
            &format!("tier2-executor-vectorized-dense-streaming-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized dense streaming join benchmark context");
    bench_expected_sql_case(group, runtime, &context, workload, scale, join_sql(2));
}

fn bench_optional_join_case(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    filters: &[String],
    context: &workloads::BenchContext,
    workload: &'static str,
    scale: &str,
    sql: &'static str,
) {
    if benchmark_enabled(filters, workload, scale) {
        bench_expected_sql_case(group, runtime, context, workload, scale, sql);
    }
}

fn bench_expected_sql_case(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    workload: &'static str,
    scale: &str,
    sql: &'static str,
) {
    black_box(runtime.block_on(workloads::execute_sql(context, sql)));
    let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, workload, scale);
    group.bench_function(
        BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
        |b| {
            b.iter(|| black_box(runtime.block_on(workloads::execute_sql(context, sql))));
        },
    );
}

fn join_sql(limit: u32) -> &'static str {
    if limit == 2 {
        "SELECT bench_join_users.name, bench_join_orders.total \
         FROM bench_join_users JOIN bench_join_orders \
         ON bench_join_users.user_key = bench_join_orders.order_user_key \
         LIMIT 2"
    } else {
        "SELECT bench_join_users.name, bench_join_orders.total \
         FROM bench_join_users JOIN bench_join_orders \
         ON bench_join_users.user_key = bench_join_orders.order_user_key \
         LIMIT 50"
    }
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_executor
}

criterion_main!(benches);
