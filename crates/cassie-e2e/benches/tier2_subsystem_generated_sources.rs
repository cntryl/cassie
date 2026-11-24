//! Compare fixed filtered-LIMIT work across generated source cardinalities.
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_generated_sources",
    );
    let cases = [("2k", 2_048), ("100k", 100_000)].map(|(scale, rows)| {
        let case = stress::StressCase::new("generated_source_filtered_limit", scale)
            .runtime_contract(
                stress::FixtureDeclaration::new(
                    performance_benchmarks::FixtureClass::Subsystem,
                    rows,
                    format!("tier2_subsystem_generated_sources/{scale}"),
                ),
                stress::OperationUnit::Query,
            );
        (case, rows)
    });
    if !cases.iter().any(|(case, _)| runner.is_enabled(case)) {
        runner.finish();
        return;
    }
    std::env::set_var("CASSIE_STORAGE_MODE", "memory");
    let mut config = CassieRuntimeConfig::from_env().expect("source benchmark config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie =
        Cassie::new_in_memory_with_config(config).expect("source benchmark memory fixture");
    cassie.startup().expect("source benchmark startup");
    let session = cassie.create_session("benchmark", None);
    for (case, rows) in cases {
        if !runner.is_enabled(&case) {
            continue;
        }
        let sql = format!(
            "SELECT value FROM generate_series(1,{rows}) s(value) WHERE value % 2 = 0 LIMIT 16"
        );
        let preflight = cassie
            .execute_sql(&session, &sql, vec![])
            .expect("bounded source preflight");
        assert_eq!(preflight.rows().len(), 16);
        runner.measure_counted(case, || {
            let result = cassie
                .execute_sql(&session, &sql, vec![])
                .expect("bounded generated query");
            assert_eq!(result.rows().len(), 16);
            cassie::benchmark::KernelObservation::new(1, 16)
        });
    }
    cassie.shutdown();
    runner.finish();
}
