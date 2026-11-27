//! Paired product evidence for the selected mapping's weight validation path.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use cassie::types::graph::{path_to_json, Path as GraphPath};
use cassie::types::{TypedValue, Value};
use serde::Serialize;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};

use super::{graph_evidence, graph_social, open, Fixture, StoreDir, SOCIAL_EDGES, SOCIAL_PEOPLE};

const WEIGHT_INDEX: &str = "CREATE INDEX bench_knows_invalid_weight ON bench_knows ((1)) \
    WHERE weight IS NULL OR weight < 0";
const WARMUPS: usize = 5;
const PAIRS: usize = 100;

#[derive(Clone, Copy, Serialize)]
struct ValidationPhases {
    validation_ns: u64,
    search_ns: u64,
    index_probes: u64,
    full_scans: u64,
}

impl ValidationPhases {
    fn snapshot(fixture: &Fixture) -> Self {
        let metrics = fixture.cassie.metrics();
        let read = |name: &str| {
            metrics["read_paths"][name]
                .as_u64()
                .unwrap_or_else(|| panic!("missing weighted graph metric {name}"))
        };
        Self {
            validation_ns: read("graph_weight_validation_ns"),
            search_ns: read("graph_weight_search_ns"),
            index_probes: read("graph_weight_validation_index_probes"),
            full_scans: read("graph_weight_validation_full_scans"),
        }
    }

    fn since(self, baseline: Self) -> Self {
        Self {
            validation_ns: self.validation_ns - baseline.validation_ns,
            search_ns: self.search_ns - baseline.search_ns,
            index_probes: self.index_probes - baseline.index_probes,
            full_scans: self.full_scans - baseline.full_scans,
        }
    }
}

#[derive(Serialize)]
struct Observation {
    latency_ms: f64,
    counters: Json,
    phases: ValidationPhases,
    instance_query_memory_high_water_bytes: u64,
}

#[derive(Serialize)]
struct Sample {
    pair: usize,
    placement: usize,
    #[serde(flatten)]
    observation: Observation,
}

fn build_fixture(store: &StoreDir, social: &graph_social::Social, indexed: bool) -> Json {
    let fixture = open(&store.0);
    let started = Instant::now();
    let plan = social.plan();
    plan.create_tables(&fixture.cassie, &fixture.session);

    let index_ms = if indexed {
        let start = Instant::now();
        graph_social::run(&fixture.cassie, &fixture.session, WEIGHT_INDEX);
        Some(start.elapsed().as_secs_f64() * 1_000.0)
    } else {
        None
    };
    let load_started = Instant::now();
    let (acknowledged_documents, successful_batches) = plan.load_rows(&fixture.cassie);
    let load_ms = load_started.elapsed().as_secs_f64() * 1_000.0;
    assert_eq!(
        i64::try_from(acknowledged_documents),
        Ok(SOCIAL_PEOPLE + SOCIAL_EDGES)
    );
    plan.define_graph(&fixture.cassie, &fixture.session);
    let shutdown_started = Instant::now();
    fixture.cassie.shutdown();
    json!({
        "weight_index_ddl": indexed.then_some(WEIGHT_INDEX),
        "weight_index_creation_ms": index_ms,
        "load_write_ms": load_ms,
        "load_acknowledged_documents": acknowledged_documents,
        "load_successful_batch_calls": successful_batches,
        "shutdown_ms": shutdown_started.elapsed().as_secs_f64() * 1_000.0,
        "total_setup_ms": started.elapsed().as_secs_f64() * 1_000.0,
    })
}

fn reopened_configuration(fixture: &Fixture) -> Json {
    let limits = fixture.cassie.runtime().limits();
    let native = fixture.cassie.midge.native_memory_configuration();
    assert_eq!(native.mode, "local");
    assert_eq!(
        u64::try_from(limits.query_memory_budget_bytes).expect("query budget fits u64"),
        graph_evidence::BUDGET_BYTES
    );
    assert!(!limits.execution_result_cache_enabled.is_enabled());
    json!({
        "native_memory_configuration": native,
        "query_memory_budget_bytes": limits.query_memory_budget_bytes,
        "query_timeout_ms": limits.query_timeout_ms,
        "result_cache": "disabled",
    })
}

fn observe(fixture: &Fixture, sql: &str, expected: &[Vec<String>], indexed: bool) -> Observation {
    let graph_before = graph_evidence::Counts::snapshot(&fixture.cassie);
    let phases_before = ValidationPhases::snapshot(fixture);
    let started = Instant::now();
    let result = fixture
        .cassie
        .execute_sql(&fixture.session, sql, Vec::new())
        .unwrap_or_else(|error| {
            panic!(
                "paired graph route failed with SQLSTATE {}",
                error.sql_state()
            )
        });
    let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let phases = ValidationPhases::snapshot(fixture).since(phases_before);
    let graph = graph_evidence::Counts::snapshot(&fixture.cassie).since(graph_before);
    assert_eq!(graph_evidence::canonical_rows(result.rows()), expected);
    if indexed {
        assert_eq!(
            (
                phases.index_probes,
                phases.full_scans,
                graph.validation_rows
            ),
            (1, 0, 0)
        );
    } else {
        let edges = u64::try_from(SOCIAL_EDGES).expect("positive fixture edges");
        assert_eq!(
            (
                phases.index_probes,
                phases.full_scans,
                graph.validation_rows
            ),
            (0, 1, edges)
        );
    }
    assert!(phases.validation_ns > 0 && phases.search_ns > 0);
    assert!(graph.index_entries > 0 && graph.neighborhoods > 0 && graph.edges_admitted > 0);
    assert_eq!(
        graph.full_rows, 0,
        "indexed expansion without full-row fallback"
    );
    assert_eq!(graph.result_cache_hits, 0, "uncached route");
    let peak = fixture.cassie.metrics()["query"]["peak_accounted_memory_bytes"]
        .as_u64()
        .expect("peak accounted query memory");
    assert!(peak <= graph_evidence::BUDGET_BYTES);
    Observation {
        latency_ms,
        counters: graph.to_json(),
        phases,
        instance_query_memory_high_water_bytes: peak,
    }
}

fn ranked_path_identity(fixture: &Fixture, route: &graph_social::Route) -> Json {
    let sql = route.sql_with_projection("w.path_rank, w.cost, w.path, w.edge_costs");
    let result = fixture
        .cassie
        .execute_sql(&fixture.session, &sql, Vec::new())
        .expect("read untimed native ranked paths");
    let mut ranked = result
        .rows()
        .iter()
        .map(|row| {
            let [Value::Int64(rank), cost, Value::Typed(TypedValue::Record(record)), edge_costs] =
                row.as_slice()
            else {
                panic!("expected rank, cost, native path and edge costs");
            };
            let path = GraphPath::from_record(record).expect("valid native path");
            let mut path = path_to_json(&path);
            // Each independently loaded fixture has exactly one vertex mapping and one edge
            // mapping. Their database, graph and mapping UUIDs differ between the fixtures.
            for group in ["vertices", "edges"] {
                for element in path[group].as_array_mut().expect("typed path elements") {
                    let object = element.as_object_mut().expect("typed graph element");
                    for id in ["database_id", "graph_id", "mapping_id"] {
                        object.remove(id).expect("graph identity field");
                    }
                }
            }
            (
                *rank,
                json!({ "rank": rank, "cost": cost, "path": path, "edge_costs": edge_costs }),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(ranked.len(), route.k);
    ranked.sort_by_key(|(rank, _)| *rank);
    json!(ranked.into_iter().map(|(_, path)| path).collect::<Vec<_>>())
}

fn assert_same_expansion(control: &Observation, indexed: &Observation) {
    for key in [
        "expanded_vertices",
        "expanded_edges",
        "index_entries",
        "row_fetches",
        "full_rows",
        "statement_cache_hits",
        "overlay_changes",
        "reference_reads",
        "result_cache_hits",
    ] {
        assert_eq!(
            control.counters[key], indexed.counters[key],
            "graph traversal {key}"
        );
    }
}

fn summarize(samples: &[Sample]) -> Json {
    let mut time = samples
        .iter()
        .map(|sample| sample.observation.latency_ms)
        .collect::<Vec<_>>();
    let mut validation = samples
        .iter()
        .map(|sample| sample.observation.phases.validation_ns)
        .collect::<Vec<_>>();
    let mut search = samples
        .iter()
        .map(|sample| sample.observation.phases.search_ns)
        .collect::<Vec<_>>();
    time.sort_by(f64::total_cmp);
    validation.sort_unstable();
    search.sort_unstable();
    let median = (samples.len() * 50).div_ceil(100) - 1;
    let p95 = (samples.len() * 95).div_ceil(100) - 1;
    json!({
        "p50_ms": time[median],
        "p95_ms": time[p95],
        "validation_p50_ns": validation[median],
        "validation_p95_ns": validation[p95],
        "search_p50_ns": search[median],
        "search_p95_ns": search[p95],
    })
}

fn interleaved_samples(
    fixtures: &[Fixture; 2],
    sql: &str,
    expected: &[Vec<String>],
) -> [Vec<Sample>; 2] {
    let mut samples = std::array::from_fn(|_| Vec::with_capacity(PAIRS));
    for pair in 0..PAIRS {
        let order = if pair % 2 == 0 { [0, 1] } else { [1, 0] };
        for (placement, index) in order.into_iter().enumerate() {
            samples[index].push(Sample {
                pair,
                placement,
                observation: observe(&fixtures[index], sql, expected, index == 1),
            });
        }
        assert_same_expansion(&samples[0][pair].observation, &samples[1][pair].observation);
    }
    samples
}

pub(super) fn run(output_dir: &Path) {
    assert_eq!(std::env::var("CASSIE_STORAGE_MODE").as_deref(), Ok("local"));
    let stores = [StoreDir::create(), StoreDir::create()];
    let social = graph_social::Social::new(SOCIAL_PEOPLE, SOCIAL_EDGES);
    let setup = [
        build_fixture(&stores[0], &social, false),
        build_fixture(&stores[1], &social, true),
    ];
    let fixtures = stores.each_ref().map(|store| open(&store.0));
    let configs = fixtures.each_ref().map(reopened_configuration);
    assert_eq!(configs[0], configs[1]);
    let route = social.route(1, super::super::ROUTE_HOPS, super::super::ROUTE_PATHS);
    let sql = route.sql();
    let expected = (1_i64..)
        .zip(&route.costs)
        .map(|(rank, cost)| vec![rank.to_string(), cost.to_string()])
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), route.k);

    let preflight = std::array::from_fn::<_, 2, _>(|index| {
        let fixture = &fixtures[index];
        let cold = observe(fixture, &sql, &expected, index == 1);
        let warm = observe(fixture, &sql, &expected, index == 1);
        (cold, warm, ranked_path_identity(fixture, &route))
    });
    let [(control_cold, control_warm, control_paths), (index_cold, index_warm, index_paths)] =
        preflight;
    assert_same_expansion(&control_cold, &index_cold);
    assert_same_expansion(&control_warm, &index_warm);
    assert_eq!(
        control_paths, index_paths,
        "complete ranked paths and edge costs"
    );
    let path_bytes = serde_json::to_vec(&control_paths).expect("serialize ranked paths");
    let path_digest =
        Sha256::digest(path_bytes)
            .iter()
            .fold(String::with_capacity(64), |mut out, byte| {
                write!(out, "{byte:02x}").expect("encode route digest");
                out
            });
    for (index, fixture) in fixtures.iter().enumerate() {
        for _ in 0..WARMUPS {
            observe(fixture, &sql, &expected, index == 1);
        }
    }

    let [control_samples, index_samples] = interleaved_samples(&fixtures, &sql, &expected);
    for fixture in &fixtures {
        fixture.cassie.shutdown();
    }
    let control_summary = summarize(&control_samples);
    let index_summary = summarize(&index_samples);
    let indexed_p95 = index_summary["p95_ms"].as_f64().expect("indexed p95");
    let receipt = json!({
        "metric": "weighted_validation_pair",
        "measurement_valid": true,
        "target": "indexed p95_ms <= 100",
        "target_met": indexed_p95 <= 100.0,
        "fixture": {
            "people": SOCIAL_PEOPLE,
            "edges": SOCIAL_EDGES,
            "mapped_rows": SOCIAL_PEOPLE + SOCIAL_EDGES,
            "snapshot": "both stores independently loaded, closed and reopened before timing",
            "configuration": configs[0],
            "cold_scope": "first query after reopen; OS page cache is retained",
            "memory_scope": "instance lifetime accounted query memory high-water",
        },
        "route": { "sql": sql, "oracle_costs": route.costs, "ranked_paths": control_paths, "ranked_paths_sha256": path_digest },
        "warmups_per_arm": WARMUPS,
        "samples_per_arm": PAIRS,
        "control": {
            "setup": setup[0], "cold": control_cold, "warm": control_warm,
            "summary": control_summary, "per_query": control_samples,
        },
        "indexed": {
            "setup": setup[1], "cold": index_cold, "warm": index_warm,
            "summary": index_summary, "per_query": index_samples,
        },
    });
    std::fs::create_dir_all(output_dir).expect("create graph pair output directory");
    let path = output_dir.join("weighted_validation_pair.json");
    let temporary = path.with_extension("json.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&receipt).expect("serialize graph pair"),
    )
    .expect("write graph pair receipt");
    std::fs::rename(&temporary, &path).expect("publish graph pair receipt");
    println!("product_graph_report={}", path.display());
}
