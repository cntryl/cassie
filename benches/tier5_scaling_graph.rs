//! Degree, depth and fanout curves under the same 64 MiB query budget.

use std::sync::Arc;
use std::time::Instant;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

#[path = "support/graph_scaling.rs"]
mod graph_scaling;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;

const OWNER: &str = "tier5_scaling_graph";
const BUDGET: usize = 64 * 1024 * 1024;

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
    for (workload, points) in [
        (
            "graph_degree_scaling_query",
            [("16", 16, 1), ("64", 64, 1), ("256", 256, 1)],
        ),
        (
            "graph_depth_scaling_query",
            [("2", 1, 2), ("4", 1, 4), ("8", 1, 8)],
        ),
        (
            "graph_fanout_scaling_query",
            [("2", 2, 3), ("4", 4, 3), ("8", 8, 3)],
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
            let setup_start = Instant::now();
            let mut config = CassieRuntimeConfig::default();
            config.limits.query_memory_budget_bytes = BUDGET;
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
            let cassie = Arc::new(
                Cassie::new_in_memory_with_config(config).expect("memory scaling fixture"),
            );
            cassie.startup().expect("scaling startup");
            let session = cassie.create_session("benchmark", None);
            tree.populate(&cassie, &session);
            let setup_ns = setup_start.elapsed().as_nanos();
            let sql = tree.sql();
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
    runner.finish();
}
