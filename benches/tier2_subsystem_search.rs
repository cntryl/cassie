const BENCHMARK: &str = "tier2_subsystem_search";
const QUERY_BATCH: u64 = 64;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "full_text_executor", dataset);
        let case =
            stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let context = runtime
            .block_on(workloads::context(&format!("tier2-search-{dataset}"), rows))
            .expect("benchmark context");
        runner.fixed_timed_count(
            case.metadata("operation_unit", "query"),
            QUERY_BATCH,
            || {
                run_sql_batch(
                    &runtime,
                    &context,
                    "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
                    QUERY_BATCH,
                )
            },
        );
    }
    bench_fulltext_temperature(&runtime, &mut runner);

    runner.finish();
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

fn bench_fulltext_temperature(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
) {
    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000), ("250k", 250_000)] {
        for temperature in ["cold", "warm"] {
            let workload = format!("full_text_{temperature}");
            let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, &workload, dataset);
            let case = stress::StressCase::fixed_operations(
                2,
                benchmark.workload,
                benchmark.fixture_scale,
            );
            if !runner.is_enabled(&case) {
                continue;
            }
            let context = runtime
                .block_on(workloads::context(
                    &format!("tier2-search-{temperature}-{dataset}"),
                    rows,
                ))
                .expect("full-text temperature benchmark context");
            let query = "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20";
            if temperature == "warm" {
                let _ = runtime.block_on(workloads::execute_sql(&context, query));
            }
            assert_fulltext_preflight(&context, query, temperature, dataset);
            runner.fixed_timed_count(
                case.metadata("operation_unit", "query"),
                QUERY_BATCH,
                || run_sql_batch(runtime, &context, query, QUERY_BATCH),
            );
        }
    }
}

fn assert_fulltext_preflight(
    context: &workloads::BenchContext,
    query: &str,
    temperature: &str,
    dataset: &str,
) {
    let before = context.cassie.metrics();
    let result = context
        .cassie
        .execute_sql(&context.session, query, vec![])
        .expect("full-text preflight query");
    assert!(
        !result.rows.is_empty(),
        "full-text {temperature} {dataset} returned no rows"
    );
    let explain = context
        .cassie
        .execute_sql(
            &context.session,
            "EXPLAIN SELECT id FROM bench_documents WHERE search(body, 'alpha') LIMIT 20",
            vec![],
        )
        .expect("full-text preflight explain");
    let explain_text = explain
        .rows
        .first()
        .and_then(|row| row.first())
        .map(|value| match value {
            cassie::types::Value::String(text) => text.clone(),
            other => format!("{other:?}"),
        })
        .unwrap_or_default();
    assert!(
        explain_text.contains("fulltext") || explain_text.contains("FullText"),
        "full-text {temperature} {dataset} did not expose a full-text access path: {explain_text}"
    );
    let after = context.cassie.metrics();
    let candidate_delta = after["search"]["candidate_count_total"]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(
            before["search"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default(),
        );
    let cache_hit_delta = after["query_cache"]["l1_hits"]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(
            before["query_cache"]["l1_hits"]
                .as_u64()
                .unwrap_or_default(),
        )
        + after["query_cache"]["l2_hits"]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(
                before["query_cache"]["l2_hits"]
                    .as_u64()
                    .unwrap_or_default(),
            );
    assert!(
        (temperature == "cold" && candidate_delta > 0)
            || (temperature == "warm" && (candidate_delta > 0 || cache_hit_delta > 0)),
        "full-text {temperature} {dataset} did not report candidate or cache evidence: before={before} after={after}"
    );
}
