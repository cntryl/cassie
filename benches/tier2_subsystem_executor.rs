const BENCHMARK: &str = "tier2_subsystem_executor";
const EXECUTOR_QUERY_BATCH: u64 = 64;

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    std::env::set_var("CASSIE_PARALLEL_AGGREGATION_WORKERS", "4");
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    bench_fixed_executor_cases(&mut runner, &runtime);
    for dataset_rows in [10_000, 100_000] {
        bench_scaled_executor_cases(&mut runner, &runtime, dataset_rows);
    }

    runner.finish();
}

fn bench_fixed_executor_cases(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
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

    let runnable = cases
        .into_iter()
        .filter(|(workload, _)| {
            runner.is_enabled(&stress::StressCase::fixed_operations(2, *workload, "10k"))
        })
        .collect::<Vec<_>>();
    if runnable.is_empty() {
        return;
    }

    let context = runtime
        .block_on(workloads::context("tier2-executor", 10_000))
        .expect("benchmark context");
    for (workload, sql) in runnable {
        bench_sql_case(runner, runtime, &context, workload, "10k", sql);
    }
}

fn bench_scaled_executor_cases(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
) {
    let scale = scale_label(dataset_rows);
    bench_column_batch_case(runner, runtime, dataset_rows, scale);
    bench_join_pair_cases(runner, runtime, dataset_rows, scale);
    bench_indexed_join_case(runner, runtime, dataset_rows, scale);
    bench_right_indexed_join_case(runner, runtime, dataset_rows, scale);
    bench_streaming_join_case(runner, runtime, dataset_rows, scale);
    bench_dense_streaming_join_case(runner, runtime, dataset_rows, scale);
    bench_late_match_join_case(runner, runtime, dataset_rows, scale);
    bench_fanout_join_case(runner, runtime, dataset_rows, scale);
}

fn scale_label(dataset_rows: usize) -> &'static str {
    if dataset_rows == 10_000 {
        "10k"
    } else {
        "100k"
    }
}

fn bench_column_batch_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "column_batch_covered_projection";
    if !enabled_expected_case(runner, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::column_batch_context(
            &format!("tier2-executor-column-{scale}"),
            dataset_rows,
        ))
        .expect("column-batch benchmark context");
    bench_expected_sql_case(
        runner,
        runtime,
        &context,
        workload,
        scale,
        "SELECT title, body FROM bench_documents WHERE status = 'approved' LIMIT 50",
    );
}

fn bench_join_pair_cases(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    if !enabled_expected_case(runner, "vectorized_join_equi", scale)
        && !enabled_expected_case(runner, "vectorized_left_join_limited", scale)
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
        runner,
        runtime,
        &context,
        "vectorized_join_equi",
        scale,
        join_sql(50),
    );
    bench_optional_join_case(
        runner,
        runtime,
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
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_indexed_inner_join";
    if !enabled_expected_case(runner, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_indexed_join_context(
            &format!("tier2-executor-vectorized-indexed-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized indexed join benchmark context");
    bench_expected_sql_case(runner, runtime, &context, workload, scale, join_sql(50));
}

fn bench_right_indexed_join_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_right_indexed_inner_join";
    if !enabled_expected_case(runner, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_right_indexed_join_context(
            &format!("tier2-executor-vectorized-right-indexed-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized right-indexed join benchmark context");
    bench_expected_sql_case(runner, runtime, &context, workload, scale, join_sql(50));
}

fn bench_streaming_join_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_streaming_inner_join";
    if !enabled_expected_case(runner, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_sparse_join_context(
            &format!("tier2-executor-vectorized-streaming-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized streaming join benchmark context");
    bench_expected_sql_case(runner, runtime, &context, workload, scale, join_sql(50));
}

fn bench_dense_streaming_join_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_dense_streaming_inner_join";
    if !enabled_expected_case(runner, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_dense_join_context(
            &format!("tier2-executor-vectorized-dense-streaming-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized dense streaming join benchmark context");
    bench_expected_sql_case(runner, runtime, &context, workload, scale, join_sql(2));
}

fn bench_late_match_join_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_late_match_inner_join";
    if !enabled_expected_case(runner, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_late_match_join_context(
            &format!("tier2-executor-vectorized-late-match-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized late-match join benchmark context");
    bench_expected_sql_case(runner, runtime, &context, workload, scale, join_sql(50));
}

fn bench_fanout_join_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    dataset_rows: usize,
    scale: &str,
) {
    let workload = "vectorized_fanout_inner_join";
    if !enabled_expected_case(runner, workload, scale) {
        return;
    }

    let context = runtime
        .block_on(workloads::vectorized_fanout_join_context(
            &format!("tier2-executor-vectorized-fanout-join-{scale}"),
            dataset_rows,
        ))
        .expect("vectorized fanout join benchmark context");
    bench_expected_sql_case(runner, runtime, &context, workload, scale, join_sql(500));
}

fn bench_optional_join_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    workload: &'static str,
    scale: &str,
    sql: &'static str,
) {
    if enabled_expected_case(runner, workload, scale) {
        bench_expected_sql_case(runner, runtime, context, workload, scale, sql);
    }
}

fn bench_expected_sql_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    workload: &'static str,
    scale: &str,
    sql: &'static str,
) {
    let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, workload, scale);
    let _ = runtime.block_on(workloads::execute_sql(context, sql));
    let query_batch = query_batch_for(workload, scale);
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale)
            .metadata("operation_unit", "query"),
        query_batch,
        || run_sql_batch(runtime, context, sql, query_batch),
    );
}

fn bench_sql_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    workload: &'static str,
    scale: &'static str,
    sql: &'static str,
) {
    let _ = runtime.block_on(workloads::execute_sql(context, sql));
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(2, workload, scale)
            .metadata("operation_unit", "query"),
        EXECUTOR_QUERY_BATCH,
        || run_sql_batch(runtime, context, sql, EXECUTOR_QUERY_BATCH),
    );
}

fn enabled_expected_case(runner: &stress::CassieStressRunner, workload: &str, scale: &str) -> bool {
    performance_benchmarks::expect_benchmark(BENCHMARK, workload, scale);
    runner.is_enabled(&stress::StressCase::fixed_operations(2, workload, scale))
}

fn query_batch_for(workload: &str, scale: &str) -> u64 {
    if scale == "10k" {
        match workload {
            "vectorized_left_join_limited" => return 128,
            "vectorized_right_indexed_inner_join" | "vectorized_streaming_inner_join" => {
                return 256;
            }
            _ => {}
        }
    }

    EXECUTOR_QUERY_BATCH
}

fn join_sql(limit: u32) -> &'static str {
    match limit {
        2 => {
            "SELECT bench_join_users.name, bench_join_orders.total \
             FROM bench_join_users JOIN bench_join_orders \
             ON bench_join_users.user_key = bench_join_orders.order_user_key \
             LIMIT 2"
        }
        50 => {
            "SELECT bench_join_users.name, bench_join_orders.total \
             FROM bench_join_users JOIN bench_join_orders \
             ON bench_join_users.user_key = bench_join_orders.order_user_key \
             LIMIT 50"
        }
        500 => {
            "SELECT bench_join_users.name, bench_join_orders.total \
             FROM bench_join_users JOIN bench_join_orders \
             ON bench_join_users.user_key = bench_join_orders.order_user_key \
             LIMIT 500"
        }
        _ => unreachable!("unsupported join limit"),
    }
}

fn run_sql_batch(
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    sql: &str,
    queries: u64,
) -> usize {
    let mut rows = 0usize;
    for _ in 0..queries {
        rows = rows.saturating_add(runtime.block_on(workloads::execute_sql(context, sql)));
    }
    std::hint::black_box(rows)
}
