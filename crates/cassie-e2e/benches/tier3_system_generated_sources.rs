use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;
use cassie_bench_support::{performance_benchmarks, stress};
use std::time::Instant;

const BENCHMARK: &str = "tier3_system_generated_sources";
const ROWS: usize = 100_000;

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier3, BENCHMARK);
    let case = stress::StressCase::new("generated_source_filtered_limit", "100k")
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Representative,
                ROWS,
                "tier3_system_generated_sources/100k",
            ),
            stress::OperationUnit::Query,
        );
    if !runner.is_enabled(&case) {
        runner.finish();
        return;
    }

    let setup_started = Instant::now();
    let mut config = CassieRuntimeConfig::from_env().expect("generated-source benchmark config");
    config.limits.execution_result_cache_enabled =
        ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config)
        .expect("generated-source benchmark memory fixture");
    cassie
        .startup()
        .expect("generated-source benchmark startup");
    let session = cassie.create_session("benchmark", None);
    let sql = format!(
        "SELECT value FROM generate_series(1,{ROWS}) s(value) WHERE value % 2 = 0 LIMIT 16"
    );
    let preflight = cassie
        .execute_sql(&session, &sql, vec![])
        .expect("generated-source preflight");
    let expected = (2_i64..=32)
        .step_by(2)
        .map(|value| vec![Value::Int64(value)])
        .collect::<Vec<_>>();
    assert_eq!(preflight.rows(), expected, "generated-source preflight rows");
    let plan = cassie
        .execute_sql(&session, &format!("EXPLAIN {sql}"), vec![])
        .expect("generated-source EXPLAIN preflight");
    let plan = format!("{:?}", plan.rows());
    assert!(plan.contains("access_path=table-function"), "{plan}");
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
    let evidence_cassie = cassie.clone();
    runner.measure_batch(
        case.metadata("setup_time_ns", setup_time_ns)
            .metadata("result_cardinality", "16")
            .preflight_evidence("table-function", "none")
            .runtime_evidence(move || evidence_cassie.metrics()),
        1,
        || {
            let result = cassie
                .execute_sql(&session, &sql, vec![])
                .expect("generated-source query");
            assert_eq!(result.rows(), expected, "timed generated-source rows");
            std::hint::black_box(result.rows().len())
        },
    );
    cassie.shutdown();
    runner.finish();
}
