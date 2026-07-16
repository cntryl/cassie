use std::cell::Cell;
use std::time::Instant;

use cassie::types::Value;

const MIXED_LOOKUP_SQL: &str = "SELECT id FROM bench_documents WHERE title = $1 LIMIT 1";
const TIER6_MAX_RESULT_ROWS: usize = 64;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier6,
        "tier6_soak_mixed",
    );
    let case = stress::StressCase::new("mixed_query_ingest_retrieval", "100k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Soak,
            100_000,
            "tier6_soak_mixed/100k",
        ),
        stress::OperationUnit::Operation,
    );
    let data_dir = if runner.is_enabled(&case) {
        std::env::set_var("BENCH_MIDGE_DISK", "1");
        let runtime = workloads::runtime();
        let setup_started = Instant::now();
        let context = runtime
            .block_on(workloads::context_with_mock_tei_embeddings(
                "tier6-soak-mixed-100k",
                100_000,
                TIER6_MAX_RESULT_ROWS,
            ))
            .expect("mixed soak fixture");
        let preflight = workloads::assert_explain_contains(
            &context,
            MIXED_LOOKUP_SQL,
            vec![Value::String("soak-marker-preflight".to_string())],
            "bench_documents_title_idx",
        );
        let nonce = Cell::new(0usize);
        let case = case
            .metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().to_string(),
            )
            .metadata("execution_result_cache_hits", "0")
            .metadata("failed_operations", "0")
            .metadata(
                "configured_max_result_rows",
                TIER6_MAX_RESULT_ROWS.to_string(),
            )
            .metadata(
                "query_memory_budget_bytes",
                workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string(),
            )
            .preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
            .runtime_evidence(context.cassie.clone());
        runner.measure_batch(case, 1, || {
            let current = nonce.get();
            nonce.set(current.wrapping_add(1));
            let result = runtime.block_on(workloads::bounded_mixed_operation(&context, current));
            assert_result_cardinality_within_bound(result)
        });
        workloads::assert_scaling_resource_bounds(&context);
        let metrics = context.cassie.metrics();
        assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
        assert_eq!(
            metrics["execution_result_cache"]["entries"].as_u64(),
            Some(0)
        );
        context.cassie.shutdown();
        let data_dir = context.data_dir.clone();
        drop(context);
        Some(data_dir)
    } else {
        None
    };
    runner.finish();
    if let Some(data_dir) = data_dir {
        std::fs::remove_dir_all(&data_dir).expect("clean up mixed soak fixture");
        assert!(!data_dir.exists(), "mixed soak fixture cleanup");
    }
}

fn assert_result_cardinality_within_bound(cardinality: usize) -> usize {
    assert!(
        cardinality <= TIER6_MAX_RESULT_ROWS,
        "mixed soak result cardinality {cardinality} exceeded configured bound {TIER6_MAX_RESULT_ROWS}"
    );
    cardinality
}
