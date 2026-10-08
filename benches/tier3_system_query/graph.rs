use std::time::{Duration, Instant};

use cassie::types::Value;

use super::{
    assert_metric_delta_bounded, assert_metric_increased, assert_query_cleanup_with_budget,
    assert_storage_read_bound, evidenced, fixture_dir, metric, metric_delta, stress, workloads,
    FIXTURE_ROWS, FIXTURE_ROWS_U64,
};

const GRAPH_SQL: &str = "SELECT node_id FROM graph_expand($1, $2, $3, $4, $5, $6, $7)";
const EXPECTED_GRAPH_NODES: [&str; 4] = ["node-1", "node-2", "node-3", "node-4"];
const GRAPH_CANDIDATE_BOUND: u64 = 8;
const GRAPH_READ_BOUND: u64 = 4 * (5 * (FIXTURE_ROWS_U64 - 1) + 1) + GRAPH_CANDIDATE_BOUND;

pub(super) fn bench_authoritative_graph_representative(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    case: Option<stress::StressCase>,
) {
    let Some(case) = case else {
        return;
    };
    let setup = Instant::now();
    let context = runtime
        .block_on(workloads::tier3_authoritative_graph_context(
            "tier3-authoritative-graph-100k",
            FIXTURE_ROWS,
        ))
        .expect("authoritative graph resource profile");
    let fixture = GraphFixture {
        _directory: fixture_dir::FixtureDir::new(context.data_dir.clone()),
        context,
    };
    let context = &fixture.context;
    let case = case
        .metadata(
            "query_memory_budget_bytes",
            workloads::AUTHORITATIVE_GRAPH_QUERY_MEMORY_BYTES.to_string(),
        )
        .metadata(
            "benchmark_resource_profile",
            workloads::AUTHORITATIVE_GRAPH_RESOURCE_PROFILE,
        );
    bench_graph_representative(runner, context, setup.elapsed(), Some(case));
    context.cassie.shutdown();
    drop(fixture);
}

fn bench_graph_representative(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: Option<stress::StressCase>,
) {
    let Some(case) = case else {
        return;
    };
    let case_setup = Instant::now();
    workloads::assert_fixture_boundaries(context, "bench_graph_nodes", "node-0", "node-99999");
    let preflight =
        workloads::assert_explain_contains(context, GRAPH_SQL, graph_params(), "operators=");
    let case = evidenced(
        case,
        context,
        fixture_setup + case_setup.elapsed(),
        preflight,
    );
    let before = context.cassie.metrics();
    runner.measure_batch(case, 1, || execute_graph_evidence(context));
    let after = context.cassie.metrics();
    assert_metric_increased(&before, &after, "graph", "traversals");
    let operations = metric_delta(&before, &after, "graph", "traversals");
    assert_eq!(
        metric_delta(&before, &after, "graph", "rows"),
        operations.saturating_mul(
            u64::try_from(EXPECTED_GRAPH_NODES.len()).expect("graph result count should fit u64"),
        ),
        "Tier 3 graph result metrics"
    );
    let read_bound = operations.saturating_mul(GRAPH_READ_BOUND);
    assert_metric_delta_bounded(&before, &after, "graph", "reads", read_bound);
    assert_metric_delta_bounded(
        &before,
        &after,
        "graph",
        "candidates",
        operations.saturating_mul(GRAPH_CANDIDATE_BOUND),
    );
    assert!(
        metric(&after, "graph", "last_reads") <= GRAPH_READ_BOUND,
        "Tier 3 graph final-path read bound"
    );
    assert!(
        metric(&after, "graph", "last_candidates") <= GRAPH_CANDIDATE_BOUND,
        "Tier 3 graph final-path candidate bound"
    );
    assert_storage_read_bound(&before, &after, read_bound.saturating_mul(2));
    assert_query_cleanup_with_budget(context, workloads::AUTHORITATIVE_GRAPH_QUERY_MEMORY_BYTES);
}

fn execute_graph_evidence(context: &workloads::BenchContext) -> usize {
    let result = context
        .cassie
        .execute_sql(&context.session, GRAPH_SQL, graph_params())
        .expect("Tier 3 graph representative query");
    let expected = EXPECTED_GRAPH_NODES
        .into_iter()
        .map(|node| vec![Value::String(node.to_string())])
        .collect::<Vec<_>>();
    assert_eq!(result.rows, expected, "Tier 3 graph expansion result");
    std::hint::black_box(result.rows.len())
}

fn graph_params() -> Vec<Value> {
    vec![
        Value::String("bench_graph".to_string()),
        Value::String("doc".to_string()),
        Value::String("node-0".to_string()),
        Value::Int64(4),
        Value::String("out".to_string()),
        Value::String("links".to_string()),
        Value::Int64(64),
    ]
}

// The engine and session drop before their directory during success and unwind.
struct GraphFixture {
    context: workloads::BenchContext,
    _directory: fixture_dir::FixtureDir,
}
