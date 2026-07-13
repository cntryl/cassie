const BENCHMARK: &str = "tier2_subsystem_hybrid";
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
            performance_benchmarks::expect_benchmark(BENCHMARK, "hybrid_executor", dataset);
        let case =
            stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let context = runtime
            .block_on(workloads::context_with_mock_tei_embeddings(
                &format!("tier2-hybrid-{dataset}"),
                rows,
            ))
            .expect("benchmark context");
        let query = "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 20";
        assert_hybrid_preflight(&context, query, dataset, rows);
        runner.fixed_timed_count(
            case.metadata("operation_unit", "query"),
            QUERY_BATCH,
            || run_sql_batch(&runtime, &context, query, QUERY_BATCH),
        );
    }

    runner.finish();
}

fn assert_hybrid_preflight(
    context: &workloads::BenchContext,
    query: &str,
    dataset: &str,
    rows: usize,
) {
    let before = context.cassie.metrics();
    let result = context
        .cassie
        .execute_sql(&context.session, query, vec![])
        .expect("hybrid preflight query");
    assert_eq!(
        result.rows.len(),
        20,
        "hybrid {dataset} must return top-k rows"
    );
    let after = context.cassie.metrics();
    let hybrid_before = &before["hybrid"];
    let hybrid_after = &after["hybrid"];
    assert!(
        hybrid_after["posting_reads_total"]
            .as_u64()
            .unwrap_or_default()
            > hybrid_before["posting_reads_total"]
                .as_u64()
                .unwrap_or_default(),
        "hybrid {dataset} did not read persisted text postings"
    );
    assert!(
        hybrid_after["ann_reads_total"].as_u64().unwrap_or_default()
            > hybrid_before["ann_reads_total"]
                .as_u64()
                .unwrap_or_default(),
        "hybrid {dataset} did not read persisted vector candidates"
    );
    assert!(
        hybrid_after["exact_reranks_total"]
            .as_u64()
            .unwrap_or_default()
            > hybrid_before["exact_reranks_total"]
                .as_u64()
                .unwrap_or_default(),
        "hybrid {dataset} did not exact-rerank source rows"
    );
    assert_eq!(
        hybrid_after["prefilter_fallback_count_total"]
            .as_u64()
            .unwrap_or_default(),
        hybrid_before["prefilter_fallback_count_total"]
            .as_u64()
            .unwrap_or_default(),
        "hybrid {dataset} fell back from the bounded path"
    );
    assert!(
        hybrid_after["candidate_count_total"]
            .as_u64()
            .unwrap_or_default()
            < u64::try_from(rows).unwrap_or(u64::MAX),
        "hybrid {dataset} candidate count was not bounded"
    );
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
