use std::time::Instant;

use cassie::types::Value;

const BENCHMARK: &str = "tier3_system_mixed_load";
const FIXTURE_ROWS: usize = 100_000;
const MIXED_QUERY_SQL: &str =
    "SELECT id FROM bench_documents WHERE status = $1 AND score >= $2 ORDER BY score DESC LIMIT 20";

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier3, BENCHMARK);
    let case = stress::StressCase::new("mixed_query_ingest_retrieval", "100k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Representative,
            FIXTURE_ROWS,
            "tier3_system_mixed_load/100k",
        ),
        stress::OperationUnit::Operation,
    );
    if !runner.is_enabled(&case) {
        runner.finish();
        return;
    }

    workloads::configure_tier3_environment();
    let runtime = workloads::runtime();
    let setup_started = Instant::now();
    let context = runtime
        .block_on(workloads::context("tier3-mixed-100k", FIXTURE_ROWS))
        .expect("Tier 3 mixed fixture");
    workloads::assert_fixture_boundaries(&context, &context.collection, "doc-0", "doc-99999");
    workloads::prepare_mixed_fixture(&context);
    let preflight = workloads::assert_explain_contains(
        &context,
        MIXED_QUERY_SQL,
        vec![Value::String("approved".to_string()), Value::Int64(90)],
        "bench_documents_status_score_idx",
    );
    let case = case
        .metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().to_string(),
        )
        .preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
        .runtime_evidence(context.cassie.clone());

    let mut nonce = 0_u64;
    runner.measure_batch(case, 1, || {
        nonce = nonce.wrapping_add(1);
        workloads::mixed_query_ingest_retrieval(&context, nonce)
    });
    workloads::assert_result_cache_disabled(&context);
    runner.finish();
}
