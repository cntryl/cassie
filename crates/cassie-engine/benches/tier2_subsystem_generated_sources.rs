#[path = "support/observation.rs"]
mod observation;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::{Cassie, CassieRuntimeConfig};
use observation::ObservationBridge;
const ROWS: usize = 2_048;
const SQL: &str = "SELECT value FROM generate_series(1,2048) s(value) WHERE value % 2 = 0 LIMIT 16";

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_generated_sources",
    );
    let case = stress::StressCase::new("generated_source_filtered_limit", "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            ROWS,
            "tier2_subsystem_generated_sources/2k",
        ),
        stress::OperationUnit::Query,
    );
    if !runner.is_enabled(&case) {
        runner.finish();
        return;
    }
    let mut config = CassieRuntimeConfig::from_env().expect("generated-source benchmark config");
    config.limits.execution_result_cache_enabled =
        cassie_engine::config::ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config)
        .expect("generated-source benchmark memory fixture");
    cassie
        .startup()
        .expect("generated-source benchmark startup");
    let session = cassie.create_session("benchmark", None);
    let preflight = cassie
        .execute_sql(&session, SQL, vec![])
        .expect("generated-source preflight");
    assert_eq!(preflight.rows().len(), 16);
    let evidence_cassie = cassie.clone();
    runner.measure_counted(
        case.runtime_evidence(move || evidence_cassie.metrics()),
        || {
            let result = cassie
                .execute_sql(&session, SQL, vec![])
                .expect("generated-source query");
            assert_eq!(result.rows().len(), 16);
            ObservationBridge(cassie_engine::benchmark::KernelObservation::new(
                1,
                u64::try_from(result.rows().len()).expect("result rows fit u64"),
            ))
        },
    );
    cassie.shutdown();
    runner.finish();
}
