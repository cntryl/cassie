//! Tier 2 mapped graph subsystem benchmarks over one small fixed graph: `GRAPH_TABLE` binding,
//! one-hop and bounded multi-hop expansion, and `graph_weighted_paths`.
#[path = "support/graph_social.rs"]
mod graph_social;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;

use std::time::Instant;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::sql::{binder, parse_statement};
use cassie::types::Value;

const BENCHMARK: &str = "tier2_subsystem_graph";
const SCALE: &str = "64";
const PEOPLE: i64 = 64;
const EDGES: i64 = 128;
const START: i64 = 1;
const ONE_HOP_SQL: &str = "SELECT t.friend FROM GRAPH_TABLE (bench_social MATCH \
     (a IS person WHERE a.id = $1) -[IS knows]-> (b IS person) COLUMNS (b.id AS friend)) AS t";
const BOUNDED_SQL: &str = "SELECT t.reached FROM GRAPH_TABLE (bench_social MATCH \
     (a IS person WHERE a.id = $1) -[IS knows]->{1,3} (b IS person) \
     COLUMNS (b.id AS reached)) AS t";
const BINDING_SQL: [&str; 4] = [
    "SELECT t.friend FROM GRAPH_TABLE (bench_social MATCH \
     (a IS person WHERE a.id = 1) -[IS knows]-> (b IS person) COLUMNS (b.id AS friend)) AS t",
    "SELECT t.reached FROM GRAPH_TABLE (bench_social MATCH \
     (a IS person WHERE a.id = 1) -[IS knows]->{1,3} (b IS person) \
     COLUMNS (b.id AS reached)) AS t",
    "SELECT t.reached FROM GRAPH_TABLE (bench_social MATCH \
     (a IS person WHERE a.id = 1) -[IS knows]->+ (b IS person) KEEP ALL ACYCLIC PATHS \
     COLUMNS (b.id AS reached)) AS t",
    "SELECT t.path FROM GRAPH_TABLE (bench_social MATCH \
     p = (a IS person WHERE a.id = 1) -[IS knows]->{1,2} (b IS person) \
     COLUMNS (p AS path)) AS t",
];

fn declared_case(workload: &'static str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, SCALE).runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            usize::try_from(PEOPLE).expect("fixture rows"),
            format!("{BENCHMARK}/{SCALE}"),
        ),
        unit,
    )
}

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier2, BENCHMARK);
    let binding = declared_case("graph_table_binding", stress::OperationUnit::Statement);
    let one_hop = declared_case("graph_table_one_hop", stress::OperationUnit::Query);
    let bounded = declared_case(
        "graph_table_bounded_expansion",
        stress::OperationUnit::Query,
    );
    let weighted = declared_case("graph_weighted_paths_small", stress::OperationUnit::Query);
    let cases = [&binding, &one_hop, &bounded, &weighted];
    if !cases.iter().any(|case| runner.is_enabled(case)) {
        runner.finish();
        return;
    }

    std::env::set_var("CASSIE_STORAGE_MODE", "memory");
    let mut config = CassieRuntimeConfig::from_env().expect("graph benchmark config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config).expect("graph benchmark fixture");
    cassie.startup().expect("graph benchmark startup");
    let session = cassie.create_session("benchmark", None);
    let setup_started = Instant::now();
    let social = graph_social::Social::new(PEOPLE, EDGES);
    for statement in social.setup() {
        graph_social::run(&cassie, &session, &statement);
    }
    let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();

    if runner.is_enabled(&binding) {
        let parsed = BINDING_SQL
            .iter()
            .map(|sql| parse_statement(sql).expect("graph benchmark SQL parses"))
            .collect::<Vec<_>>();
        runner.measure_counted(binding.metadata("setup_time_ns", &setup_time_ns), || {
            let bound = parsed
                .iter()
                .cloned()
                .map(|statement| {
                    binder::bind(statement, &cassie.catalog).expect("graph benchmark SQL binds")
                })
                .collect::<Vec<_>>();
            std::hint::black_box(&bound);
            cassie::benchmark::KernelObservation::new(
                u64::try_from(bound.len()).expect("small batch"),
                0,
            )
        });
    }
    let query = |sql: &str, rows: usize| {
        let result = cassie
            .execute_sql(&session, sql, vec![Value::Int64(START)])
            .unwrap_or_else(|error| {
                panic!("graph query failed with SQLSTATE {}", error.sql_state())
            });
        assert_eq!(result.rows().len(), rows, "graph query cardinality");
        cassie::benchmark::KernelObservation::new(1, u64::try_from(rows).expect("rows"))
    };
    if runner.is_enabled(&one_hop) {
        let expected = social.walks(START, 1, 1);
        runner.measure_counted(one_hop.metadata("setup_time_ns", &setup_time_ns), || {
            query(ONE_HOP_SQL, expected)
        });
    }
    if runner.is_enabled(&bounded) {
        let expected = social.walks(START, 1, 3);
        runner.measure_counted(bounded.metadata("setup_time_ns", &setup_time_ns), || {
            query(BOUNDED_SQL, expected)
        });
    }
    if runner.is_enabled(&weighted) {
        let route = social.route(START, 4, 3);
        let sql = route.sql();
        assert!(route.expected_rows > 0, "the route has at least one path");
        runner.measure_counted(weighted.metadata("setup_time_ns", &setup_time_ns), || {
            let result = cassie
                .execute_sql(&session, &sql, vec![])
                .unwrap_or_else(|error| {
                    panic!(
                        "weighted path query failed with SQLSTATE {}",
                        error.sql_state()
                    )
                });
            assert_eq!(result.rows().len(), route.expected_rows, "weighted rows");
            cassie::benchmark::KernelObservation::new(
                1,
                u64::try_from(route.expected_rows).expect("rows"),
            )
        });
    }
    cassie.shutdown();
    runner.finish();
}
