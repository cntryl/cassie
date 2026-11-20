//! Bounded vector index publication churn profile for HNSW and `IVFFlat`.
//!
//! Opt in with `CASSIE_BENCH_VECTOR_INDEX_CHURN=1`. For each algorithm and scale
//! it builds an index, drives a fixed number of replace-vector writes (each
//! publishes a new generation), forces a rebuild, then invalidates the manifest
//! to measure the labelled exact fallback and the time to return to indexed
//! service. One `vector_index_churn ...` line per profile reports build time,
//! write cost, rebuild time and persisted bytes, fallback duration, return to
//! indexed service and peak accounted query memory. The profile uses memory
//! storage; persisted bytes are the key and value bytes of the index artifacts.

use std::time::{Duration, Instant};

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::embeddings::VectorIndexState;

pub const ENV_FLAG: &str = "CASSIE_BENCH_VECTOR_INDEX_CHURN";
const DIMENSIONS: usize = 32;
const WRITE_CYCLES: usize = 20;
const QUERY_SAMPLES: usize = 32;
const TOP_K: usize = 10;
const SCALES: [(&str, usize); 2] = [("2k", 2_000), ("10k", 10_000)];
const ALGORITHMS: [(&str, &str); 2] = [
    (
        "hnsw",
        "index_type = hnsw, m = 16, ef_construction = 64, ef_search = 64",
    ),
    (
        "ivfflat",
        "index_type = ivfflat, lists = 16, probes = 4, training_sample_size = 512, training_seed = 42",
    ),
];

fn vector(row: usize, salt: usize) -> Vec<f32> {
    let mut state = (row as u64)
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add((salt as u64).wrapping_mul(1_442_695_040_888_963_407))
        | 1;
    (0..DIMENSIONS)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            f32::from(u16::try_from((state >> 48) & 0xFFFF).unwrap_or(0)) / 32_768.0 - 1.0
        })
        .collect()
}

fn literal(vector: &[f32]) -> String {
    let parts = vector
        .iter()
        .map(f32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("[{parts}]")
}

fn new_cassie() -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("benchmark runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-bench".to_string(),
        dimensions: DIMENSIONS,
    });
    config.limits.execution_result_cache_enabled =
        cassie::config::ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
    let root = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &root,
            "CREATE EXTENSION IF NOT EXISTS vector VERSION '0.8.7'",
            Vec::new(),
        )
        .expect("install vector extension");
    cassie
}

fn run(cassie: &Cassie, sql: &str) {
    cassie
        .execute_sql(&cassie.create_session("benchmark", None), sql, Vec::new())
        .expect("benchmark statement");
}

fn query_sql(sample: usize) -> String {
    format!(
        "SELECT id, vector_distance(embedding, '{}') AS distance FROM churn_docs \
         ORDER BY distance ASC LIMIT {TOP_K}",
        literal(&vector(sample * 7_919 + 3, 99))
    )
}

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples
        .get(
            samples
                .len()
                .saturating_mul(95)
                .div_ceil(100)
                .saturating_sub(1),
        )
        .copied()
        .unwrap_or_default()
}

fn query_p95(cassie: &Cassie) -> Duration {
    p95((0..QUERY_SAMPLES)
        .map(|sample| {
            let started = Instant::now();
            run(cassie, &query_sql(sample));
            started.elapsed()
        })
        .collect())
}

fn metric(cassie: &Cassie, section: &str, name: &str) -> u64 {
    cassie.metrics()[section][name].as_u64().unwrap_or_default()
}

fn footprint(cassie: &Cassie) -> serde_json::Value {
    cassie
        .midge
        .vector_index_artifact_footprint_for_diagnostics("churn_docs", "embedding")
        .expect("index footprint")
}

fn rebuild(cassie: &Cassie, algorithm: &str) {
    let collection = cassie
        .catalog
        .get_schema("churn_docs")
        .map_or_else(|| "churn_docs".to_string(), |schema| schema.collection);
    if algorithm == "hnsw" {
        cassie
            .midge
            .refresh_hnsw_indexes_for_collection(&collection)
    } else {
        cassie
            .midge
            .refresh_ivfflat_indexes_for_collection(&collection)
    }
    .expect("rebuild vector index");
}

fn measure(algorithm: &str, options: &str, scale: &str, rows: usize) {
    let cassie = new_cassie();
    run(
        &cassie,
        &format!("CREATE TABLE churn_docs (body TEXT, embedding VECTOR({DIMENSIONS}))"),
    );
    let documents = (0..rows)
        .map(|row| {
            (
                Some(format!("row-{row:06}")),
                serde_json::json!({"body": format!("row-{row:06}"), "embedding": vector(row, 0)}),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents("churn_docs", documents)
        .expect("seed churn rows");
    let build_started = Instant::now();
    run(
        &cassie,
        &format!(
            "CREATE INDEX churn_docs_idx ON churn_docs USING vector (embedding) \
             WITH (source_field = body, metric = l2, {options})"
        ),
    );
    let build = build_started.elapsed();
    let indexed = footprint(&cassie);
    let indexed_p95 = query_p95(&cassie);

    let write_started = Instant::now();
    for cycle in 0..WRITE_CYCLES {
        let row = (cycle * 7_919) % rows;
        cassie
            .midge
            .put_document(
                "churn_docs",
                Some(format!("row-{row:06}")),
                serde_json::json!({
                    "body": format!("row-{row:06}"),
                    "embedding": vector(row, cycle + 1)
                }),
            )
            .expect("replace-vector write");
    }
    let writes = write_started.elapsed();

    rebuild(&cassie, algorithm);
    let rebuild_micros = metric(&cassie, "vector", "index_last_rebuild_micros");
    let rebuild_bytes = metric(&cassie, "vector", "index_last_rebuild_bytes_written");
    let rebuild_transactions = metric(&cassie, "vector", "index_last_rebuild_transactions");
    let churned = footprint(&cassie);

    // Invalidate the manifest, then time the labelled exact fallback and the
    // return to indexed service (rebuild plus the first indexed answer).
    cassie
        .midge
        .put_vector_index_state(
            "churn_docs",
            "embedding",
            VectorIndexState {
                built_generation: 0,
                hnsw_graph: None,
                ivfflat_training: None,
            },
        )
        .expect("invalidate manifest");
    let fallbacks = metric(&cassie, "vector", &format!("{algorithm}_fallbacks"));
    let fallback_started = Instant::now();
    run(&cassie, &query_sql(0));
    let fallback_query = fallback_started.elapsed();
    let fallback_micros = metric(&cassie, "vector", "ann_last_fallback_micros");
    let fell_back =
        metric(&cassie, "vector", &format!("{algorithm}_fallbacks")).saturating_sub(fallbacks);
    let recovery_started = Instant::now();
    rebuild(&cassie, algorithm);
    run(&cassie, &query_sql(1));
    let recovery = recovery_started.elapsed();
    let peak_memory = metric(&cassie, "query", "peak_accounted_memory_bytes");

    println!(
        "vector_index_churn algorithm={algorithm} scale={scale} rows={rows} \
         dimensions={DIMENSIONS} build_ms={} index_bytes={} indexed_query_p95_us={} \
         write_cycles={WRITE_CYCLES} write_total_ms={} rebuild_us={rebuild_micros} \
         rebuild_bytes_written={rebuild_bytes} rebuild_transactions={rebuild_transactions} \
         index_bytes_after_churn={} fallback_labelled={fell_back} fallback_query_us={} \
         fallback_exact_us={fallback_micros} return_to_indexed_service_us={} \
         peak_query_memory_bytes={peak_memory}",
        build.as_millis(),
        indexed["total_bytes"],
        indexed_p95.as_micros(),
        writes.as_millis(),
        churned["total_bytes"],
        fallback_query.as_micros(),
        recovery.as_micros(),
    );
}

/// Runs the churn profile for both algorithms at 2k and 10k rows when opted in.
pub fn run_profiles() {
    if std::env::var_os(ENV_FLAG).is_none() {
        return;
    }
    for (scale, rows) in SCALES {
        for (algorithm, options) in ALGORITHMS {
            measure(algorithm, options, scale, rows);
        }
    }
}
