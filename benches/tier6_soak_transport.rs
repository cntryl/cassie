use std::time::{Duration, Instant};

use cassie::types::Value;

const TRANSPORT_QUERY_SQL: &str =
    "SELECT id, title FROM bench_documents WHERE title = $1 ORDER BY id ASC LIMIT 20";
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
        "tier6_soak_transport",
    );
    let case = stress::StressCase::new("transport_lifecycle", "10k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Soak,
            10_000,
            "tier6_soak_transport/10k",
        ),
        stress::OperationUnit::Operation,
    );
    if !runner.is_enabled(&case) {
        runner.finish();
        return;
    }

    let generated_http_tls =
        workloads::configure_http_tls().expect("configure benchmark REST TLS identity");
    let generated_http_tls_directory = generated_http_tls
        .as_ref()
        .map(|material| material.directory().to_path_buf());
    let runtime = workloads::runtime();
    let setup_started = Instant::now();
    let context = runtime
        .block_on(workloads::scalar_context(
            "tier6-soak-transport-10k",
            10_000,
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
            TIER6_MAX_RESULT_ROWS,
        ))
        .expect("transport soak fixture");
    let preflight = workloads::assert_explain_contains(
        &context,
        TRANSPORT_QUERY_SQL,
        vec![Value::String("title-1".to_string())],
        "bench_documents_title_idx",
    );
    let pgwire = runtime
        .block_on(workloads::pgwire_transport_for_context(&context))
        .expect("transport soak pgwire listener");
    let http = runtime
        .block_on(workloads::http_transport_context(&context))
        .expect("transport soak HTTP listener");
    let case = case
        .metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().to_string(),
        )
        .metadata("execution_result_cache_hits", "0")
        .metadata("failed_operations", "0")
        .metadata("max_active_sessions", "4")
        .metadata(
            "configured_max_result_rows",
            TIER6_MAX_RESULT_ROWS.to_string(),
        )
        .metadata(
            "query_memory_budget_bytes",
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string(),
        )
        .metadata("result_cardinality", "43")
        .preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
        .runtime_evidence(context.cassie.clone());

    runner.record_external(case, |sample_duration| {
        run_transport_sample(&runtime, &context, &pgwire, &http, sample_duration)
    });

    runtime
        .block_on(http.shutdown())
        .expect("shutdown transport soak HTTP listener");
    runtime
        .block_on(pgwire.shutdown())
        .expect("shutdown transport soak pgwire listener");
    runtime.block_on(workloads::wait_for_pgwire_session_cleanup(&context));
    let metrics = context.cassie.metrics();
    assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
    assert_eq!(
        metrics["runtime"]["active_operator_workers"].as_u64(),
        Some(0)
    );
    assert_eq!(metrics["pgwire"]["active_sessions"].as_u64(), Some(0));
    assert_eq!(
        metrics["execution_result_cache"]["entries"].as_u64(),
        Some(0)
    );
    context.cassie.shutdown();
    let data_dir = context.data_dir.clone();
    drop(context);
    runner.finish();
    if data_dir.is_dir() {
        std::fs::remove_dir_all(&data_dir).expect("clean up transport soak fixture directory");
    } else if data_dir.exists() {
        std::fs::remove_file(&data_dir).expect("clean up transport soak fixture marker");
    }
    assert!(!data_dir.exists(), "transport soak fixture cleanup");
    cleanup_generated_http_tls(generated_http_tls, generated_http_tls_directory);
}

fn cleanup_generated_http_tls(
    material: Option<workloads::GeneratedHttpTlsMaterial>,
    directory: Option<std::path::PathBuf>,
) {
    if let Some(material) = material {
        material
            .cleanup()
            .expect("clean up generated REST TLS identity");
    }
    if let Some(directory) = directory {
        assert!(!directory.exists(), "transport soak TLS cleanup");
    }
}

fn run_transport_sample(
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    pgwire: &workloads::PgwireTransportBenchContext,
    http: &workloads::HttpBenchContext,
    sample_duration: Duration,
) -> stress::ExternalSample {
    let started = Instant::now();
    let mut completed = 0_u64;
    loop {
        let rows = assert_result_cardinality_within_bound(
            runtime.block_on(workloads::pgwire_transport_extended_query(pgwire)),
        );
        assert_eq!(rows, 20);
        let http_operations = assert_result_cardinality_within_bound(
            runtime.block_on(workloads::http_transport_document_create_get(http)),
        );
        assert_eq!(http_operations, 3);
        let churn_rows = assert_result_cardinality_within_bound(
            runtime.block_on(workloads::pgwire_transport_connection_churn(pgwire)),
        );
        assert_eq!(churn_rows, 20, "churn query result cardinality");
        assert_result_cardinality_within_bound(
            rows.saturating_add(http_operations)
                .saturating_add(churn_rows),
        );
        completed = completed.saturating_add(5);
        assert_resource_bounds(context);
        if started.elapsed() >= sample_duration {
            break;
        }
    }
    stress::ExternalSample::new(started.elapsed(), completed)
}

fn assert_result_cardinality_within_bound(cardinality: usize) -> usize {
    assert!(
        cardinality <= TIER6_MAX_RESULT_ROWS,
        "transport soak result cardinality {cardinality} exceeded configured bound {TIER6_MAX_RESULT_ROWS}"
    );
    cardinality
}

fn assert_resource_bounds(context: &workloads::BenchContext) {
    let metrics = context.cassie.metrics();
    assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(0));
    assert_eq!(
        metrics["execution_result_cache"]["entries"].as_u64(),
        Some(0)
    );
    assert!(
        metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .unwrap_or_default()
            <= u64::try_from(workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES)
                .expect("query-memory budget should fit u64")
    );
    assert_eq!(
        metrics["runtime"]["active_operator_workers"].as_u64(),
        Some(0)
    );
    assert!(
        metrics["pgwire"]["active_sessions"]
            .as_u64()
            .unwrap_or_default()
            <= 4
    );
}
