//! #1263 measurement only: actual nullable primitive transport and aggregate paths.
use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[path = "tier3_system_typed_primitive/evidence.rs"]
mod evidence;
#[path = "tier3_system_typed_primitive/fixture.rs"]
mod fixture;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;

const BENCHMARK: &str = "tier3_system_typed_primitive";

fn declaration(case: fixture::Case) -> stress::StressCase {
    stress::StressCase::new(case.name, "100k")
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Representative,
                fixture::ROWS,
                fixture::FIXTURE,
            ),
            stress::OperationUnit::Query,
        )
        .metadata("storage_mode", "memory")
        .metadata("authority", case.authority)
        .metadata("covering_column_index", case.derived_index.to_string())
        .metadata(
            "query_memory_budget_bytes",
            fixture::QUERY_BYTES.to_string(),
        )
        .metadata(
            "native_memory_budget_bytes",
            fixture::NATIVE_BYTES.to_string(),
        )
        .metadata(
            "native_memtable_size_bytes",
            fixture::MEMTABLE_BYTES.to_string(),
        )
        .metadata("worker_count", "1")
        .metadata("result_cache", "disabled")
        .metadata("timed_private_trace_collection", "disabled")
}
fn benchmark() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier3, BENCHMARK);
    let selected = fixture::CASES
        .into_iter()
        .filter_map(|case| {
            let declaration = declaration(case);
            runner
                .is_enabled(&declaration)
                .then_some((case, declaration))
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        runner.finish();
        return;
    }
    let started = Instant::now();
    let db = fixture::Db::seed();
    let setup_time = started.elapsed().as_nanos().to_string();
    let mut receipts = Vec::new();
    for (case, declaration) in selected {
        let sql = case.sql();
        let expected = case.expected();
        let (access, fallback, explain) = evidence::preflight(&db, case);
        let receipt = evidence::observe(&db, case, &explain);
        receipts.push(receipt);
        evidence::write("benchmark", &receipts);
        evidence::assert_valid(receipts.last().expect("diagnostic receipt"));
        let native = db.cassie.midge.native_memory_configuration();
        let declaration = declaration
            .metadata("setup_time_ns", &setup_time)
            .metadata("result_cardinality", "1")
            .metadata("resolved_native_total_bytes", native.total.to_string())
            .metadata(
                "resolved_native_memtable_limit_bytes",
                native.memtable_limit.to_string(),
            )
            .metadata(
                "resolved_native_flush_threshold_bytes",
                native.flush_threshold.to_string(),
            )
            .metadata(
                "resolved_native_transaction_pool_bytes",
                native.transaction_pool.to_string(),
            )
            .preflight_evidence(access, fallback)
            .runtime_evidence(Arc::clone(&db.cassie));
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
            runner.measure_batch(declaration, 1, || {
                db.checked(&sql, &expected);
                std::hint::black_box(1_usize)
            });
        });
    }
    runner.finish();
}
fn cpu_profile() {
    let requested = std::env::var("BENCH_TYPED_PRIMITIVE_CASE")
        .expect("CPU profile requires an exact workload");
    let case = fixture::CASES
        .into_iter()
        .find(|case| case.name == requested)
        .expect("one of six declared workloads");
    let seconds = std::env::var("BENCH_TYPED_PRIMITIVE_PROFILE_SECONDS")
        .map_or(Ok(30), |value| value.parse::<u64>())
        .expect("positive profile duration");
    assert!(
        (1..=600).contains(&seconds),
        "profile duration must be 1..=600 seconds"
    );
    let db = fixture::Db::seed();
    let sql = case.sql();
    let expected = case.expected();
    let (_, _, explain) = evidence::preflight(&db, case);
    let diagnostic = evidence::observe(&db, case, &explain);
    evidence::write("cpu-profile", std::slice::from_ref(&diagnostic));
    evidence::assert_valid(&diagnostic);
    let ready = serde_json::json!({"pid":std::process::id(),"executable":std::env::current_exe().expect("benchmark executable"),"workload":case.name,"seconds":seconds,"fixture":fixture::FIXTURE,"timed_private_trace_collection":false,"canonical_timing":false,"phase":"setup_complete_query_window_next"});
    println!("typed_primitive_cpu_ready={ready}");
    std::io::stdout().flush().expect("profile ready marker");
    let started = Instant::now();
    let mut queries = 0_u64;
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        while started.elapsed() < Duration::from_secs(seconds) {
            db.checked(&sql, &expected);
            queries += 1;
        }
    });
    let complete = serde_json::json!({"phase":"query_window_complete","workload":case.name,"pid":std::process::id(),"queries":queries,"actual_window_ns":started.elapsed().as_nanos().to_string(),"canonical_timing":false,"allocation_counts":"unavailable_without_external_profiler"});
    println!("typed_primitive_cpu_complete={complete}");
    evidence::write("cpu-profile", &[diagnostic, ready, complete]);
}
fn main() {
    evidence::validate_failure_classification();
    match std::env::var("BENCH_TYPED_PRIMITIVE_MODE")
        .unwrap_or_else(|_| "benchmark".into())
        .as_str()
    {
        "benchmark" => benchmark(),
        "cpu-profile" => cpu_profile(),
        other => panic!("unsupported primitive measurement mode: {other}"),
    }
}
