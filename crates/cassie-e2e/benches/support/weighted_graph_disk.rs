//! Local storage counter evidence for the fixed Tier 3 weighted route.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

use super::{
    collect_weighted_evidence, fixture_identity, graph_evidence, graph_social, workloads, Fixture,
    SOCIAL_EDGES, SOCIAL_INDEXES, SOCIAL_PEOPLE,
};

#[path = "weighted_graph_disk_checks.rs"]
mod checks;

#[path = "weighted_graph_disk_pair.rs"]
mod pair;

#[path = "weighted_graph_product_status.rs"]
mod product_status;

struct StoreDir(PathBuf);

impl StoreDir {
    fn create() -> Self {
        let path = std::env::temp_dir().join(format!("cassie-graph-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).expect("create isolated graph store");
        Self(path)
    }
}

impl Drop for StoreDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "cannot clean isolated graph store {}: {error}",
                self.0.display()
            );
        }
    }
}

fn open(path: &Path) -> Fixture {
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.query_memory_budget_bytes = workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
    config.limits.query_timeout_ms = workloads::LARGE_ANALYTICAL_BENCHMARK_QUERY_TIMEOUT_MS;
    let cassie = Cassie::new_local_with_data_dir_and_config(path, config)
        .expect("open local weighted graph store");
    cassie.startup().expect("start local weighted graph store");
    let session = cassie.create_session("benchmark", None);
    Fixture {
        cassie: Arc::new(cassie),
        session,
    }
}

pub(super) fn run() {
    run_inner(None);
}

pub(super) fn run_product(profile: &str, output_dir: &Path) {
    match std::env::var("CASSIE_BENCH_GRAPH_DISK_VARIANT") {
        Err(std::env::VarError::NotPresent) => run_inner(Some((profile, output_dir))),
        Ok(variant) if variant == "paired_weight_validation" => {
            assert_eq!(
                profile, "full",
                "weight validation pair uses the full fixture"
            );
            pair::run(output_dir);
        }
        other => panic!("unsupported graph disk variant {other:?}"),
    }
}

fn run_inner(product: Option<(&str, &Path)>) {
    let store = StoreDir::create();
    let (people, edges, scale) = match product.map(|(profile, _)| profile) {
        Some("smoke") => (1_000, 2_500, "smoke"),
        Some("full") | None => (SOCIAL_PEOPLE, SOCIAL_EDGES, "100k"),
        Some(other) => panic!("unsupported graph product profile {other:?}"),
    };
    let social = graph_social::Social::new(people, edges);
    {
        let fixture = open(&store.0);
        social.plan().apply(&fixture.cassie, &fixture.session);
        fixture.cassie.shutdown();
    }
    let fixture = open(&store.0);
    let limits = fixture.cassie.runtime().limits();
    let native = fixture.cassie.midge.native_memory_configuration();
    assert_eq!(native.mode, "local", "reopened graph storage mode");
    assert_eq!(
        u64::try_from(limits.query_memory_budget_bytes).expect("query budget fits u64"),
        graph_evidence::BUDGET_BYTES,
        "reopened graph query budget"
    );
    assert!(!limits.execution_result_cache_enabled.is_enabled());
    let mapped_rows = people + edges;
    let mut identity = fixture_identity(mapped_rows, SOCIAL_INDEXES);
    identity["scale"] = serde_json::json!(scale);
    identity["storage_mode"] = serde_json::json!(native.mode);
    identity["native_memory_configuration"] = serde_json::json!(native);
    identity["snapshot"] = serde_json::json!("unchanged committed fixture after close and reopen");
    identity["cold_scope"] = serde_json::json!("first query on reopened engine");
    identity["warm_scope"] = serde_json::json!("same query immediately afterward");
    identity["query_timeout_ms"] = serde_json::json!(limits.query_timeout_ms);

    let mut evidence = graph_evidence::Evidence::new("tier3_system_graph_disk_reopen");
    let (cold, warm) = collect_weighted_evidence(&mut evidence, &fixture, &social, identity, scale);
    checks::should_validate_and_expand_the_weighted_graph_after_reopen(
        &mut evidence,
        u64::try_from(edges).expect("positive fixture edge count"),
        cold,
        warm,
    );
    let reopen_valid = [cold, warm].into_iter().all(|counts| {
        counts.validation_rows == u64::try_from(edges).expect("positive fixture edge count")
            && counts.index_entries > 0
            && counts.neighborhoods > 0
            && counts.edges_admitted > 0
            && counts.full_rows == 0
            && counts.result_cache_hits == 0
    });
    if let Some((profile, output_dir)) = product {
        let reopened = ReopenedGraph {
            people,
            edges,
            cold,
            warm,
            valid: reopen_valid,
        };
        measure_routes(&fixture, &social, &reopened, profile, output_dir);
    }
    fixture.cassie.shutdown();
    evidence.finish();
}

struct ReopenedGraph {
    people: i64,
    edges: i64,
    cold: graph_evidence::Counts,
    warm: graph_evidence::Counts,
    valid: bool,
}

fn sample_routes(
    fixture: &Fixture,
    route: &graph_social::Route,
    expected_edges: i64,
) -> Vec<serde_json::Value> {
    let mut expected = (1..)
        .zip(&route.costs)
        .map(|(rank, cost): (i64, &i64)| vec![rank.to_string(), cost.to_string()])
        .collect::<Vec<_>>();
    expected.sort();
    let sql = route.sql();
    for _ in 0..5 {
        let result = fixture
            .cassie
            .execute_sql(&fixture.session, &sql, Vec::new())
            .unwrap_or_else(|error| {
                panic!("graph warmup failed with SQLSTATE {}", error.sql_state())
            });
        assert_eq!(graph_evidence::canonical_rows(result.rows()), expected);
    }
    let mut outcomes = Vec::with_capacity(100);
    for query in 0..100 {
        let before = graph_evidence::Counts::snapshot(&fixture.cassie);
        let started = Instant::now();
        let result = fixture
            .cassie
            .execute_sql(&fixture.session, &sql, Vec::new())
            .unwrap_or_else(|error| {
                panic!(
                    "graph product query failed with SQLSTATE {}",
                    error.sql_state()
                )
            });
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let counts = graph_evidence::Counts::snapshot(&fixture.cassie).since(before);
        let matches_oracle = graph_evidence::canonical_rows(result.rows()) == expected;
        assert!(
            matches_oracle,
            "weighted route differs from independent oracle"
        );
        assert_eq!(
            counts.validation_rows,
            u64::try_from(expected_edges).expect("edges")
        );
        assert_eq!(counts.full_rows, 0);
        assert_eq!(counts.result_cache_hits, 0);
        outcomes.push(serde_json::json!({
            "query": query,
            "latency_ms": elapsed_ms,
            "matches_oracle": matches_oracle,
            "returned_paths": result.rows().len(),
            "fallback": counts.full_rows > 0 || counts.result_cache_hits > 0,
            "counters": counts.to_json(),
        }));
    }
    outcomes
}

fn measure_routes(
    fixture: &Fixture,
    social: &graph_social::Social,
    reopened: &ReopenedGraph,
    profile: &str,
    output_dir: &Path,
) {
    let route = social.route(1, super::ROUTE_HOPS, super::ROUTE_PATHS);
    let outcomes = sample_routes(fixture, &route, reopened.edges);
    let mut samples = outcomes
        .iter()
        .map(|outcome| outcome["latency_ms"].as_f64().expect("latency"))
        .collect::<Vec<_>>();
    samples.sort_by(f64::total_cmp);
    let p95_ms = samples[(samples.len() * 95).div_ceil(100) - 1];
    let (measurement_valid, target_met) =
        product_status::measurement_status(profile, p95_ms, reopened.valid, &outcomes);
    let metric = serde_json::json!({
        "metric": "weighted_routes",
        "profile": profile,
        "measurement_valid": measurement_valid,
        "target_met": target_met,
        "target": "p95_ms <= 100",
        "measurements": {
            "fixture": {
                "scale": if profile == "full" { "100k" } else { "smoke" },
                "mapped_rows": reopened.people + reopened.edges,
                "people": reopened.people,
                "edges": reopened.edges,
                "storage_mode": "local",
                "query_memory_budget_bytes": graph_evidence::BUDGET_BYTES,
                "result_cache": "disabled",
                "snapshot": "closed and reopened before timing",
            },
            "route": {
                "source": route.source,
                "target": route.target,
                "maximum_hops": route.max_hops,
                "path_limit": route.k,
                "oracle_costs": route.costs,
            },
            "edge_rows": reopened.edges,
            "warmups": 5,
            "queries": outcomes.len(),
            "reopen_oracle_valid": reopened.valid,
            "reopened_counters": {"cold": reopened.cold.to_json(), "warm": reopened.warm.to_json()},
            "p50_ms": samples[(samples.len() * 50).div_ceil(100) - 1],
            "p95_ms": p95_ms,
            "per_query": outcomes,
        },
        "failures": if reopened.valid { vec![] } else { vec!["reopen counter oracle failed"] },
    });
    std::fs::create_dir_all(output_dir).expect("create product output directory");
    let path = output_dir.join("weighted_routes.json");
    let temporary = path.with_extension("json.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&metric).expect("serialize graph receipt"),
    )
    .expect("write graph receipt");
    std::fs::rename(&temporary, &path).expect("publish graph receipt");
    println!("product_graph_report={}", path.display());
}
