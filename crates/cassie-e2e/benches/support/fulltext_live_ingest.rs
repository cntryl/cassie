//! Paired bulk-build versus live-index ingest profile for full-text maintenance.
//!
//! Opt in with `CASSIE_BENCH_FULLTEXT_LIVE_INGEST=1`. For each scale it loads the
//! same generated documents twice: once creating the index after loading (bulk
//! build) and once creating it first so every write maintains it (live ingest).
//! One `fulltext_live_ingest ...` line per profile reports write throughput, p95
//! query latency, index bytes per update, forced-rebuild time and the stale-index
//! fallback window. Peak memory is not measured by this harness.

use std::time::{Duration, Instant};

use cassie::app::Cassie;
use cassie::midge::adapter::set_fulltext_maintenance_failure_point;
use cassie::types::Value;

use super::workloads;

pub const ENV_FLAG: &str = "CASSIE_BENCH_FULLTEXT_LIVE_INGEST";
const INSERT_BATCH_ROWS: usize = 250;
const QUERY_SAMPLES: usize = 64;
const UPDATE_SAMPLES: usize = 100;
const VOCABULARY: u64 = 2_000;
const WORDS_PER_DOCUMENT: usize = 12;
const QUERY_TERMS: [&str; 4] = ["w17", "w233", "w1999", "w5"];

#[derive(Clone, Copy)]
enum Profile {
    BulkBuild,
    LiveIngest,
}

impl Profile {
    fn label(self) -> &'static str {
        match self {
            Self::BulkBuild => "bulk_build",
            Self::LiveIngest => "live_ingest",
        }
    }
}

fn document_body(row: usize) -> String {
    let mut state = (row as u64).wrapping_mul(6_364_136_223_846_793_005) ^ 0x9e37_79b9;
    (0..WORDS_PER_DOCUMENT)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            format!("w{}", (state >> 33) % VOCABULARY)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn document_key(row: usize) -> String {
    format!("doc-{row:08}")
}

fn run(context: &workloads::BenchContext, sql: &str, params: Vec<Value>) {
    context
        .cassie
        .execute_sql(&context.session, sql, params)
        .unwrap_or_else(|error| {
            let statement: String = sql.chars().take(120).collect();
            panic!("benchmark statement failed: {statement}: {error}")
        });
}

/// Loads rows in keyed batches through the storage layer, as a projector does.
fn insert_rows(context: &workloads::BenchContext, rows: usize) {
    let collection = collection_of(context);
    for start in (0..rows).step_by(INSERT_BATCH_ROWS) {
        let end = (start + INSERT_BATCH_ROWS).min(rows);
        let documents = (start..end)
            .map(|row| {
                (
                    Some(document_key(row)),
                    serde_json::json!({"id": row, "body": document_body(row)}),
                )
            })
            .collect();
        context
            .cassie
            .midge
            .put_documents(&collection, documents)
            .expect("batch load");
    }
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

fn query_p95(context: &workloads::BenchContext) -> Duration {
    p95((0..QUERY_SAMPLES)
        .map(|sample| {
            let started = Instant::now();
            run(
                context,
                "SELECT _id, search_score(body, $1) AS score FROM live_ingest_docs \
                 WHERE search(body, $1) ORDER BY score DESC LIMIT 10",
                vec![Value::String(
                    QUERY_TERMS[sample % QUERY_TERMS.len()].to_string(),
                )],
            );
            started.elapsed()
        })
        .collect())
}

fn metric(cassie: &Cassie, name: &str) -> u64 {
    cassie.metrics()["search"][name]
        .as_u64()
        .unwrap_or_default()
}

fn average_update_index_bytes(context: &workloads::BenchContext, rows: usize) -> u64 {
    let before = metric(
        &context.cassie,
        "fulltext_maintenance_index_bytes_written_total",
    );
    let collection = collection_of(context);
    for sample in 0..UPDATE_SAMPLES {
        let row = (sample * 7_919) % rows;
        // A projector replaces one document by key through the storage layer.
        context
            .cassie
            .midge
            .put_document(
                &collection,
                Some(document_key(row)),
                serde_json::json!({"id": row, "body": document_body(rows + sample)}),
            )
            .expect("single-document update");
    }
    metric(
        &context.cassie,
        "fulltext_maintenance_index_bytes_written_total",
    )
    .saturating_sub(before)
        / u64::try_from(UPDATE_SAMPLES).unwrap_or(1)
}

/// Time the index serves stale (labelled fallback) after an interrupted
/// publication until maintenance-debt retry rebuilds it.
fn fallback_window(context: &workloads::BenchContext, rows: usize) -> Duration {
    let started = Instant::now();
    set_fulltext_maintenance_failure_point(true);
    context
        .cassie
        .midge
        .put_document(
            &collection_of(context),
            Some("fallback-probe".to_string()),
            serde_json::json!({"id": -1, "body": document_body(rows + UPDATE_SAMPLES)}),
        )
        .expect("interrupted publication write");
    context
        .cassie
        .midge
        .retry_maintenance_debt()
        .expect("retry maintenance debt");
    started.elapsed()
}

fn collection_of(context: &workloads::BenchContext) -> String {
    context
        .cassie
        .catalog
        .get_schema("live_ingest_docs")
        .map_or_else(
            || "live_ingest_docs".to_string(),
            |schema| schema.collection,
        )
}

fn measure(runtime: &tokio::runtime::Runtime, scale: &str, rows: usize, profile: Profile) {
    let context = runtime
        .block_on(workloads::empty_context(&format!(
            "tier5-fulltext-{}-{scale}",
            profile.label()
        )))
        .expect("empty benchmark context");
    run(
        &context,
        "CREATE TABLE live_ingest_docs (id INT NOT NULL, body TEXT)",
        Vec::new(),
    );
    let create_index = "CREATE INDEX live_ingest_docs_ft ON live_ingest_docs USING fulltext (body)";
    let started = Instant::now();
    let build_time = match profile {
        Profile::BulkBuild => {
            insert_rows(&context, rows);
            let index_started = Instant::now();
            run(&context, create_index, Vec::new());
            index_started.elapsed()
        }
        Profile::LiveIngest => {
            run(&context, create_index, Vec::new());
            insert_rows(&context, rows);
            Duration::ZERO
        }
    };
    let ingest = started.elapsed();
    let query_p95 = query_p95(&context);
    let update_bytes = average_update_index_bytes(&context, rows);
    let rebuild_started = Instant::now();
    context
        .cassie
        .midge
        .rebuild_fulltext_index_for_diagnostics(&collection_of(&context), "live_ingest_docs_ft")
        .expect("forced rebuild");
    let rebuild = rebuild_started.elapsed();
    let window = fallback_window(&context, rows);
    println!(
        "fulltext_live_ingest profile={} scale={scale} rows={rows} ingest_ms={} \
         write_rows_per_s={:.0} index_build_ms={} query_p95_us={} \
         index_bytes_per_update={update_bytes} rebuild_ms={} fallback_window_ms={}",
        profile.label(),
        ingest.as_millis(),
        cassie::types::numeric::usize_to_f64(rows) / ingest.as_secs_f64().max(f64::EPSILON),
        build_time.as_millis(),
        query_p95.as_micros(),
        rebuild.as_millis(),
        window.as_millis(),
    );
}

/// Runs the paired profiles at 10k and 100k rows when opted in.
pub fn run_paired_profiles(runtime: &tokio::runtime::Runtime) {
    if std::env::var_os(ENV_FLAG).is_none() {
        return;
    }
    for (scale, rows) in [("10k", 10_000), ("100k", 100_000)] {
        for profile in [Profile::BulkBuild, Profile::LiveIngest] {
            measure(runtime, scale, rows, profile);
        }
    }
}
