//! Degree, depth and fanout curves under the same 64 MiB query budget.

use std::sync::Arc;
use std::time::Instant;

use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

#[path = "support/graph_edge_list.rs"]
mod graph_edge_list;
#[path = "support/graph_evidence.rs"]
mod graph_evidence;
#[path = "support/graph_scaling.rs"]
mod graph_scaling;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;

const OWNER: &str = "tier5_scaling_graph";
const BUDGET: usize = 64 * 1024 * 1024;

/// A freshly opened in-memory database under the 64 MiB budget with result caching disabled.
fn open() -> (Arc<Cassie>, CassieSession) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = BUDGET;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(config).expect("memory scaling fixture"));
    cassie.startup().expect("scaling startup");
    let session = cassie.create_session("benchmark", None);
    (cassie, session)
}

/// Runs one edge-list case on its own fresh database and returns its cold and warm counters.
fn edge_list_case(
    evidence: &mut graph_evidence::Evidence,
    list: &graph_edge_list::EdgeList,
    (family, case): (&str, &str),
    (start, hops): (i64, usize),
    fixture: serde_json::Value,
) -> (graph_evidence::Counts, graph_evidence::Counts) {
    let (cassie, session) = open();
    list.populate(&cassie, &session);
    let counts = evidence.collect(
        &cassie,
        &session,
        &graph_evidence::Spec {
            family,
            case,
            sql: &graph_edge_list::EdgeList::sql(start, hops, hops),
            params: &[],
            expected: graph_evidence::int_rows(list.walk_endpoints(start, hops, hops)),
            exact_switch: true,
            fixture,
        },
    );
    cassie.shutdown();
    counts
}

/// The skewed-hub cases and the disconnected-growth pair. They are counter evidence only:
/// they are not registered timing scenarios.
fn extra_evidence(evidence: &mut graph_evidence::Evidence) {
    let skew = graph_edge_list::skewed_hub();
    let skew_fixture = |note: &str| {
        serde_json::json!({
            "seed": format!("{:#x}", graph_edge_list::SEED), "vertices": skew.vertices,
            "edges": skew.edges.len(), "hub_degree": graph_edge_list::HUB_DEGREE,
            "ordinary_degree": graph_edge_list::ORDINARY_DEGREE, "feeder": graph_edge_list::FEEDER,
            "storage_mode": "memory", "indexes": "el_link(src), el_link(dst)",
            "query_memory_budget_bytes": BUDGET, "result_cache": "disabled", "note": note,
        })
    };
    for (case, start, hops, note) in [
        (
            "hub-anchored",
            1,
            1,
            "the query starts at the hub: pays the full hub degree",
        ),
        (
            "ordinary-avoids-hub",
            2,
            2,
            "two hops from an ordinary vertex that cannot expand the hub",
        ),
        (
            "ordinary-through-hub",
            graph_edge_list::FEEDER,
            2,
            "the feeder reaches the hub, which is expanded once",
        ),
    ] {
        edge_list_case(
            evidence,
            &skew,
            ("graph_skewed_hub", case),
            (start, hops),
            skew_fixture(note),
        );
    }
    let mut touched = Vec::new();
    for (case, rings) in [
        ("1x", graph_edge_list::RINGS_AT_1X),
        ("10x", 10 * graph_edge_list::RINGS_AT_1X),
    ] {
        let list = graph_edge_list::disconnected(rings);
        let fixture = serde_json::json!({
            "neighbourhood": "fanout-4 depth-3 tree rooted at vertex 1, held fixed",
            "disconnected_rings": rings, "ring_size": graph_edge_list::RING_SIZE,
            "vertices": list.vertices, "edges": list.edges.len(), "data": "closed-form, no seed",
            "storage_mode": "memory", "indexes": "el_link(src), el_link(dst)",
            "query_memory_budget_bytes": BUDGET, "result_cache": "disabled",
        });
        let (cold, warm) = edge_list_case(
            evidence,
            &list,
            ("graph_disconnected_growth", case),
            (
                1,
                usize::try_from(graph_edge_list::TREE_DEPTH).expect("depth"),
            ),
            fixture,
        );
        let key = |c: &graph_evidence::Counts| {
            [
                c.index_entries,
                c.row_fetches,
                c.full_rows,
                c.edges_admitted,
                c.neighborhoods,
            ]
        };
        touched.push((key(&cold), key(&warm)));
    }
    if touched[0] != touched[1] {
        evidence.finding(format!(
            "graph_disconnected_growth: 10x touched {:?}, 1x touched {:?}",
            touched[1], touched[0]
        ));
    }
}

/// Counter evidence of one rooted-tree point against the oracle over its generated edges.
fn tree_evidence(
    evidence: &mut graph_evidence::Evidence,
    cassie: &Cassie,
    session: &CassieSession,
    (workload, scale): (&str, &str),
    tree: &graph_scaling::Tree,
) {
    let oracle = graph_edge_list::EdgeList {
        vertices: i64::try_from(tree.nodes).expect("nodes"),
        edges: tree.edges(),
    };
    evidence.collect(
        cassie,
        session,
        &graph_evidence::Spec {
            family: workload,
            case: scale,
            sql: &tree.sql(),
            params: &[],
            expected: graph_evidence::int_rows(oracle.walk_endpoints(1, tree.depth, tree.depth)),
            exact_switch: true,
            fixture: serde_json::json!({
                "fanout": tree.fanout, "depth": tree.depth, "mapped_rows": tree.mapped_rows(),
                "data": "closed-form rooted tree, no seed", "storage_mode": "memory",
                "indexes": "scaling_link(src), scaling_link(dst)",
                "query_memory_budget_bytes": BUDGET, "result_cache": "disabled",
            }),
        },
    );
}

fn main() {
    if let Some(profile) =
        std::env::var_os("CASSIE_BENCH_DEPLOYMENT_PROFILE_ID").filter(|profile| !profile.is_empty())
    {
        assert_eq!(
            profile, "local-dev-graph-topology-memory-64m",
            "graph topology fixtures require their actual memory deployment profile"
        );
    }
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier5, OWNER);
    let mut evidence = graph_evidence::Evidence::new(OWNER);
    let mut ran = false;
    for (workload, points) in [
        (
            "graph_degree_scaling_query",
            [
                ("degree-16", 16, 1),
                ("degree-64", 64, 1),
                ("degree-256", 256, 1),
            ],
        ),
        (
            "graph_depth_scaling_query",
            [("depth-2", 1, 2), ("depth-4", 1, 4), ("depth-8", 1, 8)],
        ),
        (
            "graph_fanout_scaling_query",
            [("fanout-2", 2, 3), ("fanout-4", 4, 3), ("fanout-8", 8, 3)],
        ),
    ] {
        for (scale, fanout, depth) in points {
            let tree = graph_scaling::Tree::new(fanout, depth);
            let case = stress::StressCase::new(workload, scale).runtime_contract(
                stress::FixtureDeclaration::new(
                    performance_benchmarks::FixtureClass::Scaling,
                    tree.mapped_rows(),
                    format!("{OWNER}/{workload}/{scale}"),
                ),
                stress::OperationUnit::Query,
            );
            if !runner.is_enabled(&case) {
                continue;
            }
            ran = true;
            let setup_start = Instant::now();
            let (cassie, session) = open();
            tree.populate(&cassie, &session);
            let setup_ns = setup_start.elapsed().as_nanos();
            let sql = tree.sql();
            tree_evidence(&mut evidence, &cassie, &session, (workload, scale), &tree);
            let plan = cassie
                .execute_sql(&session, &format!("EXPLAIN {sql}"), vec![])
                .expect("scaling preflight");
            let Some(Value::String(plan)) = plan.rows().first().and_then(|row| row.first()) else {
                panic!("scaling explain plan");
            };
            assert!(plan.contains("=indexed("));
            let case = case
                .metadata("setup_time_ns", setup_ns.to_string())
                .metadata("query_memory_budget_bytes", BUDGET.to_string())
                .metadata("graph_mapped_rows", tree.mapped_rows().to_string())
                .metadata("graph_fanout", fanout.to_string())
                .metadata("graph_depth", depth.to_string())
                .metadata("graph_expected_rows", tree.leaves.to_string())
                .metadata("graph_explain", plan)
                .preflight_evidence("graph_table_indexed_read", "none")
                .runtime_evidence(cassie.clone());
            runner.measure_batch(case, 1, || {
                let result = cassie
                    .execute_sql(&session, &sql, vec![])
                    .expect("scaling query");
                assert_eq!(result.rows().len(), tree.leaves, "rooted-tree leaf oracle");
                std::hint::black_box(result.rows().len())
            });
            cassie.shutdown();
        }
    }
    if ran {
        extra_evidence(&mut evidence);
    }
    runner.finish();
    evidence.finish();
}
