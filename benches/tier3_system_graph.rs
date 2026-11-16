//! Tier 3 mapped graph representatives: projector-shaped workloads over ordinary tables, each
//! mapped by a property graph that copies no data. Fixtures select memory storage and the
//! standard analytical query-memory profile.
use std::sync::Arc;
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

#[path = "support/graph_org.rs"]
mod graph_org;
#[path = "support/graph_social.rs"]
mod graph_social;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const BENCHMARK: &str = "tier3_system_graph";
/// The scenario contract fixes the Tier 3 fixture class at 100,000 rows. The exact read of a
/// mapped graph does not fit the 64 MiB analytical profile at that size (it fails with
/// `54000` from roughly 30,000 mapped rows), so the fixtures below use endpoint indexes and
/// the lazy read path, and the result metadata records the real mapped count as
/// `graph_mapped_rows`.
const FIXTURE_SCALE: &str = "100k";
const FIXTURE_ROWS: usize = 100_000;
const SOCIAL_MAPPED_ROWS: i64 = SOCIAL_PEOPLE + SOCIAL_EDGES;
const HIERARCHY_MAPPED_ROWS: i64 = 2 * HIERARCHY_NODES - 1;
const ACCESS_MAPPED_ROWS: i64 =
    ACCESS_PRINCIPALS + ACCESS_RESOURCES + ACCESS_MEMBERSHIPS + ACCESS_RESOURCES;
const SOCIAL_PEOPLE: i64 = 30_000;
const SOCIAL_EDGES: i64 = 70_000;
const HIERARCHY_NODES: i64 = 50_000;
const ACCESS_PRINCIPALS: i64 = 20_000;
const ACCESS_GROUPS: i64 = 1_000;
const ACCESS_MEMBERSHIPS: i64 = 40_000;
const ACCESS_RESOURCES: i64 = 20_000;
const MINIMUM_REACHABLE_GRANTS: usize = 3;
const ROUTE_HOPS: usize = 4;
const ROUTE_PATHS: usize = 3;
const QUERIES_PER_BATCH: u64 = 4;
const FRIENDS_OF_FRIENDS_SQL: &str = "SELECT t.friend FROM GRAPH_TABLE (bench_social MATCH \
     (a IS person WHERE a.id = $1) -[IS knows]->{2,2} (b IS person) \
     COLUMNS (b.id AS friend)) AS t";
const EXACT_READ_PATH: &str = "graph_table_exact_read";
const INDEXED_READ_PATH: &str = "graph_table_indexed_read";

struct Fixture {
    cassie: Arc<Cassie>,
    session: CassieSession,
}

fn selected_case(
    runner: &stress::CassieStressRunner,
    workload: &'static str,
) -> Option<stress::StressCase> {
    let case = stress::StressCase::new(workload, FIXTURE_SCALE)
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Representative,
                FIXTURE_ROWS,
                "tier3_system_graph/100k",
            ),
            stress::OperationUnit::Query,
        )
        .metadata(
            "query_memory_budget_bytes",
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string(),
        );
    runner.is_enabled(&case).then_some(case)
}

/// Native storage memory for the memory-mode fixtures. It is separate from the 64 MiB query
/// budget and large enough to hold the generated tables without write stalls.
const STORAGE_MEMORY_BYTES: usize = 2 * 1024 * 1024 * 1024;
const STORAGE_MEMTABLE_BYTES: usize = 512 * 1024 * 1024;

fn open_fixture() -> Fixture {
    std::env::set_var("CASSIE_STORAGE_MODE", "memory");
    let mut config = CassieRuntimeConfig::from_env().expect("graph benchmark config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.storage_memory_budget_bytes = Some(STORAGE_MEMORY_BYTES);
    config.storage_memtable_size_bytes = Some(STORAGE_MEMTABLE_BYTES);
    config.limits.query_memory_budget_bytes = workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
    config.limits.query_timeout_ms = workloads::LARGE_ANALYTICAL_BENCHMARK_QUERY_TIMEOUT_MS;
    let cassie = Cassie::new_in_memory_with_config(config).expect("graph benchmark fixture");
    cassie.startup().expect("graph benchmark startup");
    let session = cassie.create_session("benchmark", None);
    Fixture {
        cassie: Arc::new(cassie),
        session,
    }
}

/// Reads the access path from the graph node of `EXPLAIN`. A plan without that node (the
/// weighted path function) is classified by one counted execution: the read is `indexed`
/// when it read index entries and no full rows.
fn preflight(fixture: &Fixture, sql: &str, params: &[Value]) -> (String, &'static str) {
    let plan = fixture
        .cassie
        .execute_sql(&fixture.session, &format!("EXPLAIN {sql}"), params.to_vec())
        .unwrap_or_else(|error| panic!("graph EXPLAIN failed with SQLSTATE {}", error.sql_state()));
    let text = match plan.rows().first().and_then(|row| row.first()) {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    };
    // The graph node names the access path of each mapping: every edge mapping indexed
    // means neighborhoods are read through endpoint indexes; any exact mapping is a full read.
    if !text.contains("graph_table=") {
        return counted_path(fixture, sql, params);
    }
    let path = if text.contains("=indexed(") && !text.contains("=exact(") {
        INDEXED_READ_PATH
    } else {
        EXACT_READ_PATH
    };
    (path.to_owned(), "reported")
}

/// Runs `sql` once and classifies its read path from the graph read counters it moved.
fn counted_path(fixture: &Fixture, sql: &str, params: &[Value]) -> (String, &'static str) {
    let counter = |name: &str| {
        fixture.cassie.metrics()["read_paths"][name]
            .as_u64()
            .unwrap_or_else(|| panic!("graph read counter {name} is missing"))
    };
    let (entries, full) = (counter("graph_index_entries"), counter("graph_full_rows"));
    fixture
        .cassie
        .execute_sql(&fixture.session, sql, params.to_vec())
        .unwrap_or_else(|error| {
            panic!("graph preflight failed with SQLSTATE {}", error.sql_state())
        });
    let indexed = counter("graph_index_entries") > entries && counter("graph_full_rows") == full;
    let path = if indexed {
        INDEXED_READ_PATH
    } else {
        EXACT_READ_PATH
    };
    (path.to_owned(), "counted")
}

fn evidenced(
    case: stress::StressCase,
    fixture: &Fixture,
    setup: Duration,
    mapped_rows: i64,
    (path, source): (String, &'static str),
) -> stress::StressCase {
    case.metadata("setup_time_ns", setup.as_nanos().max(1).to_string())
        .metadata("graph_mapped_rows", mapped_rows.to_string())
        .metadata("graph_access_path_source", source)
        .preflight_evidence(path, "none")
        .runtime_evidence(fixture.cassie.clone())
}

/// Runs the query `QUERIES_PER_BATCH` times and checks the oracle cardinality of every result.
fn expect_rows(fixture: &Fixture, sql: &str, params: &[Value], rows: usize) -> usize {
    let mut observed = 0;
    for _ in 0..QUERIES_PER_BATCH {
        let result = fixture
            .cassie
            .execute_sql(&fixture.session, sql, params.to_vec())
            .unwrap_or_else(|error| {
                panic!("graph query failed with SQLSTATE {}", error.sql_state())
            });
        assert_eq!(
            result.rows().len(),
            rows,
            "graph representative cardinality"
        );
        observed = std::hint::black_box(result.rows().len());
    }
    observed
}

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier3, BENCHMARK);
    let authorization = selected_case(&runner, "graph_authorization_reachability_query");
    let hierarchy = selected_case(&runner, "graph_hierarchy_ancestry_query");
    let friends = selected_case(&runner, "graph_friends_of_friends_query");
    let routes = selected_case(&runner, "graph_weighted_routes_query");
    if authorization.is_none() && hierarchy.is_none() && friends.is_none() && routes.is_none() {
        runner.finish();
        return;
    }

    let fixture = open_fixture();
    let setup_started = Instant::now();
    let social = graph_social::Social::new(SOCIAL_PEOPLE, SOCIAL_EDGES);
    if friends.is_some() || routes.is_some() {
        social.plan().apply(&fixture.cassie, &fixture.session);
    }
    let access = graph_org::Access::new(
        ACCESS_PRINCIPALS,
        ACCESS_GROUPS,
        ACCESS_MEMBERSHIPS,
        ACCESS_RESOURCES,
    );
    if authorization.is_some() {
        access.plan().apply(&fixture.cassie, &fixture.session);
    }
    let org = graph_org::Hierarchy::new(HIERARCHY_NODES);
    if hierarchy.is_some() {
        org.plan().apply(&fixture.cassie, &fixture.session);
    }
    let setup = setup_started.elapsed();

    if let Some(case) = authorization {
        let (user, rows) = access.sample_user(MINIMUM_REACHABLE_GRANTS);
        let params = vec![Value::Int64(user)];
        let case = evidenced(
            case,
            &fixture,
            setup,
            ACCESS_MAPPED_ROWS,
            preflight(&fixture, graph_org::REACHABILITY_SQL, &params),
        );
        runner.measure_batch(case, QUERIES_PER_BATCH, || {
            expect_rows(&fixture, graph_org::REACHABILITY_SQL, &params, rows)
        });
    }
    if let Some(case) = hierarchy {
        let (leaf, rows) = org.deepest();
        let params = vec![Value::Int64(leaf)];
        let case = evidenced(
            case,
            &fixture,
            setup,
            HIERARCHY_MAPPED_ROWS,
            preflight(&fixture, graph_org::ANCESTRY_SQL, &params),
        );
        runner.measure_batch(case, QUERIES_PER_BATCH, || {
            expect_rows(&fixture, graph_org::ANCESTRY_SQL, &params, rows)
        });
    }
    if let Some(case) = friends {
        let rows = social.walks(1, 2, 2);
        let params = vec![Value::Int64(1)];
        let case = evidenced(
            case,
            &fixture,
            setup,
            SOCIAL_MAPPED_ROWS,
            preflight(&fixture, FRIENDS_OF_FRIENDS_SQL, &params),
        );
        runner.measure_batch(case, QUERIES_PER_BATCH, || {
            expect_rows(&fixture, FRIENDS_OF_FRIENDS_SQL, &params, rows)
        });
    }
    if let Some(case) = routes {
        let route = social.route(1, ROUTE_HOPS, ROUTE_PATHS);
        let sql = route.sql();
        let case = evidenced(
            case,
            &fixture,
            setup,
            SOCIAL_MAPPED_ROWS,
            preflight(&fixture, &sql, &[]),
        );
        runner.measure_batch(case, QUERIES_PER_BATCH, || {
            expect_rows(&fixture, &sql, &[], route.expected_rows)
        });
    }
    fixture.cassie.shutdown();
    runner.finish();
}
