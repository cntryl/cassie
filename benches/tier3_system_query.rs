const BENCHMARK: &str = "tier3_system_query";
const GRAPH_EXPAND_SQL: &str =
    "SELECT node_id FROM graph_expand('bench_graph', 'doc', 'node-0', 4, 'out', 'links', 64)";
const BASE_CASES: &[(&str, &str, &str)] = &[
    (
        "simple_sql_query",
        "10k",
        "SELECT id, title FROM bench_documents WHERE id = 'doc-1'",
    ),
    (
        "indexed_filter_query",
        "10k",
        "SELECT id FROM bench_documents WHERE score = 1",
    ),
    (
        "range_query",
        "10k",
        "SELECT id FROM bench_documents WHERE score >= 10 LIMIT 100",
    ),
    (
        "sort_limit_query",
        "10k",
        "SELECT id FROM bench_documents ORDER BY score DESC LIMIT 50",
    ),
    (
        "mixed_order_scalar_query",
        "10k",
        "SELECT id FROM bench_documents WHERE status = 'approved' AND score >= 10 ORDER BY status DESC, score ASC LIMIT 50",
    ),
    (
        "expression_index_query",
        "10k",
        "SELECT id FROM bench_documents WHERE lower(title) = 'title-1' LIMIT 50",
    ),
    (
        "expression_index_range_query",
        "10k",
        "SELECT id FROM bench_documents WHERE lower(title) >= 'title-4' AND lower(title) < 'title-9' LIMIT 50",
    ),
    (
        "expression_index_order_query",
        "10k",
        "SELECT id FROM bench_documents ORDER BY lower(title) ASC LIMIT 50",
    ),
    (
        "fulltext_search_query",
        "10k",
        "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
    ),
    (
        "vector_search_query",
        "10k",
        "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20",
    ),
    (
        "hybrid_search_query",
        "10k",
        "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 20",
    ),
];
const SCALAR_100K_CASES: &[(&str, &str)] = &[
    (
        "mixed_order_scalar_query",
        "SELECT id FROM bench_documents WHERE status = 'approved' AND score >= 10 ORDER BY status DESC, score ASC LIMIT 50",
    ),
    (
        "expression_index_query",
        "SELECT id FROM bench_documents WHERE lower(title) = 'title-1' LIMIT 50",
    ),
    (
        "expression_index_range_query",
        "SELECT id FROM bench_documents WHERE lower(title) >= 'title-4' AND lower(title) < 'title-9' LIMIT 50",
    ),
    (
        "expression_index_order_query",
        "SELECT id FROM bench_documents ORDER BY lower(title) ASC LIMIT 50",
    ),
];
const MIXED_DIRECTION_SCALAR_SQL: &str =
    "SELECT id FROM bench_documents ORDER BY score DESC, id ASC LIMIT 50";

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    bench_base_10k_cases(&mut runner, &runtime);
    bench_simple_100k_case(&mut runner, &runtime);
    bench_scalar_100k_cases(&mut runner, &runtime);
    bench_mixed_direction_scalar_case(&mut runner, &runtime, "10k", 10_000);
    bench_mixed_direction_scalar_case(&mut runner, &runtime, "100k", 100_000);
    bench_time_series_case(&mut runner, &runtime, "10k", 10_000);
    bench_graph_case(&mut runner, &runtime, "10k", 10_000);
    bench_graph_case(&mut runner, &runtime, "100k", 100_000);
    bench_time_series_case(&mut runner, &runtime, "100k", 100_000);

    runner.finish();
}

fn bench_base_10k_cases(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
) {
    let runnable = BASE_CASES
        .iter()
        .copied()
        .filter(|(name, dataset, _)| should_run_case(runner, name, dataset))
        .collect::<Vec<_>>();
    if runnable.is_empty() {
        return;
    }

    let context = runtime
        .block_on(workloads::context("tier3-query", 10_000))
        .expect("benchmark context");
    for (name, dataset, sql) in runnable {
        if requires_manifest_check(name) {
            performance_benchmarks::expect_benchmark(BENCHMARK, name, dataset);
        }
        warm_and_register_sql_case(runner, runtime, &context, name, dataset, sql);
    }
}

fn requires_manifest_check(name: &str) -> bool {
    matches!(
        name,
        "simple_sql_query"
            | "mixed_order_scalar_query"
            | "mixed_direction_scalar_query"
            | "expression_index_query"
            | "expression_index_range_query"
            | "expression_index_order_query"
    )
}

fn bench_simple_100k_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
) {
    if !should_run_case(runner, "simple_sql_query", "100k") {
        return;
    }

    let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, "simple_sql_query", "100k");
    let context = runtime
        .block_on(workloads::unindexed_context("tier3-query-100k", 100_000))
        .expect("100k benchmark context");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, benchmark.workload, benchmark.fixture_scale),
        || {
            runtime.block_on(workloads::execute_sql(
                &context,
                "SELECT id, title FROM bench_documents WHERE id = 'doc-1'",
            ))
        },
    );
}

fn bench_scalar_100k_cases(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
) {
    let runnable = SCALAR_100K_CASES
        .iter()
        .copied()
        .filter(|(workload, _)| should_run_case(runner, workload, "100k"))
        .collect::<Vec<_>>();
    if runnable.is_empty() {
        return;
    }

    let context = runtime
        .block_on(workloads::scalar_context(
            "tier3-query-scalar-100k",
            100_000,
        ))
        .expect("100k scalar benchmark context");
    for (workload, sql) in runnable {
        let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, workload, "100k");
        let _ = runtime.block_on(workloads::execute_sql(&context, sql));
        runner.fixed_operations(
            stress::StressCase::fixed_operations(3, benchmark.workload, benchmark.fixture_scale),
            || runtime.block_on(workloads::execute_sql(&context, sql)),
        );
    }
}

fn bench_mixed_direction_scalar_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    scale: &str,
    rows: usize,
) {
    if !should_run_case(runner, "mixed_direction_scalar_query", scale) {
        return;
    }

    let benchmark =
        performance_benchmarks::expect_benchmark(BENCHMARK, "mixed_direction_scalar_query", scale);
    let label = format!("tier3-query-mixed-direction-{scale}");
    let context = runtime
        .block_on(workloads::scalar_context(&label, rows))
        .expect("mixed-direction scalar benchmark context");
    let _ = runtime.block_on(workloads::execute_sql(&context, MIXED_DIRECTION_SCALAR_SQL));
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, benchmark.workload, benchmark.fixture_scale),
        || runtime.block_on(workloads::execute_sql(&context, MIXED_DIRECTION_SCALAR_SQL)),
    );
}

fn bench_time_series_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    scale: &str,
    rows: usize,
) {
    if !should_run_case(runner, "time_series_window_scan", scale) {
        return;
    }

    let label = if scale == "10k" {
        "tier3-query-ts".to_string()
    } else {
        "tier3-query-ts-100k".to_string()
    };
    let context = runtime
        .block_on(workloads::time_series_context(&label, rows))
        .expect("time-series benchmark context");
    let benchmark =
        performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_window_scan", scale);
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, benchmark.workload, benchmark.fixture_scale),
        || runtime.block_on(workloads::time_series_window_scan(&context)),
    );
}

fn bench_graph_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    scale: &str,
    rows: usize,
) {
    if !should_run_case(runner, "graph_expand_query", scale) {
        return;
    }

    let label = if scale == "10k" {
        "tier3-query-graph".to_string()
    } else {
        "tier3-query-graph-100k".to_string()
    };
    let context = runtime
        .block_on(workloads::graph_context(&label, rows))
        .expect("graph benchmark context");
    let benchmark =
        performance_benchmarks::expect_benchmark(BENCHMARK, "graph_expand_query", scale);
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, benchmark.workload, benchmark.fixture_scale),
        || runtime.block_on(workloads::execute_sql(&context, GRAPH_EXPAND_SQL)),
    );
}

fn warm_and_register_sql_case(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    workload: &'static str,
    scale: &'static str,
    sql: &'static str,
) {
    let _ = runtime.block_on(workloads::execute_sql(context, sql));
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, workload, scale),
        || runtime.block_on(workloads::execute_sql(context, sql)),
    );
}

fn should_run_case(
    runner: &stress::CassieStressRunner,
    workload: &str,
    fixture_scale: &str,
) -> bool {
    runner.is_enabled(&stress::StressCase::fixed_operations(
        3,
        workload,
        fixture_scale,
    ))
}
