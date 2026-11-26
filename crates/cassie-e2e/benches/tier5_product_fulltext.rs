#[path = "support/product_mixed.rs"]
mod product;
#[path = "support/workloads.rs"]
mod workloads;

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use product::{FixedRateReport, PgwireFixture};
use tokio::task::JoinSet;

const TABLE: &str = "product_fulltext_docs";
const VOCABULARY: u64 = 2_000;
const WORDS: usize = 12;
const TERMS: [&str; 4] = ["w17", "w233", "w1999", "w5"];
const BATCH: usize = 250;
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

fn main() {
    let profile = product::profile();
    let output = product::output_dir();
    let windows = if profile.name == "smoke" { 1 } else { 3 };
    let mut reports = Vec::with_capacity(windows);
    for window in 0..windows {
        reports.push(run_window(profile, window));
    }
    let mut query_latency_us = Vec::new();
    let mut write_latency_us = Vec::new();
    let mut valid = true;
    for report in &reports {
        query_latency_us.extend_from_slice(&report.query_latency_us);
        write_latency_us.extend_from_slice(&report.writer.raw_latency_us);
        valid &= report.writer.scheduled == report.writer.offered
            && report.writer.generator_late == 0
            && report.writer.offered == report.writer.acknowledged
            && report.ranked_queries > 0
            && report.indexed_execution
            && report.ranked_oracle_preflight;
    }
    let p95 = percentile_us(&query_latency_us, 95);
    let acknowledged_rate = reports
        .iter()
        .map(|report| report.writer.acknowledged_at_window_end)
        .sum::<u64>();
    let acknowledged_rate = cassie::types::numeric::u64_to_f64(acknowledged_rate)
        / (cassie::types::numeric::usize_to_f64(windows) * profile.duration.as_secs_f64());
    let raw_artifact = "full_text_top10_raw.json";
    product::write_json(
        &output,
        raw_artifact,
        &serde_json::json!({"raw_query_latency_us": &query_latency_us, "raw_writer_latency_us": &write_latency_us, "windows": &reports}),
    );
    product::write_json(
        &output,
        "full_text_top10.json",
        &serde_json::json!({
            "metric": "full_text_top10",
            "profile": profile.name,
            "measurement_valid": valid,
            "target_met": (profile.name == "full" && valid).then_some(p95 <= 20_000 && acknowledged_rate >= 2_000.0),
            "measurements": {
                "query_p95_us": p95,
                "write_p95_us": percentile_us(&write_latency_us, 95),
                "acknowledged_documents_per_second": acknowledged_rate,
                "queries": query_latency_us.len(),
                "windows": reports.iter().map(|report| serde_json::json!({
                    "scheduled": report.writer.scheduled,
                    "offered": report.writer.offered,
                    "acknowledged": report.writer.acknowledged,
                    "acknowledged_at_window_end": report.writer.acknowledged_at_window_end,
                    "backlog_at_window_end": report.writer.backlog_at_window_end,
                    "acknowledged_after_window": report.writer.acknowledged_after_window,
                    "drain_millis": report.writer.drain_millis,
                    "ranked_queries": report.ranked_queries,
                    "indexed_execution": report.indexed_execution,
                    "index_bytes_written": report.index_bytes_written,
                    "ranked_oracle_preflight": report.ranked_oracle_preflight,
                })).collect::<Vec<_>>(),
                "raw_artifact": raw_artifact,
            },
            "failures": if valid { Vec::<String>::new() } else { vec!["fixed-rate generator, ranked result, or indexed execution check failed".to_owned()] },
        }),
    );
    assert!(
        valid,
        "full-text workload evidence failed its correctness checks"
    );
}

#[derive(serde::Serialize)]
struct WindowReport {
    writer: FixedRateReport,
    query_latency_us: Vec<u64>,
    ranked_queries: usize,
    indexed_execution: bool,
    index_bytes_written: u64,
    ranked_oracle_preflight: bool,
}

fn run_window(profile: product::ProductProfile, window: usize) -> WindowReport {
    let runtime = workloads::runtime();
    let context = runtime
        .block_on(workloads::empty_disk_context_with_temp_budget(
            &format!("product-fulltext-{window}"),
            profile.seeded_rows.saturating_add(10_000),
            64 * 1024 * 1024,
        ))
        .expect("full-text fixture");
    context
        .cassie
        .execute_sql(
            &context.session,
            &format!("CREATE TABLE {TABLE} (id INT PRIMARY KEY, body TEXT NOT NULL)"),
            Vec::new(),
        )
        .expect("create full-text table");
    context
        .cassie
        .execute_sql(
            &context.session,
            &format!("CREATE INDEX product_fulltext_idx ON {TABLE} USING fulltext (body)"),
            Vec::new(),
        )
        .expect("create full-text index");
    let collection = context
        .cassie
        .catalog
        .get_schema(TABLE)
        .map(|schema| schema.collection)
        .unwrap_or_else(|| TABLE.to_owned());
    seed_documents(&context, &collection, profile.seeded_rows);

    let fixture = runtime.block_on(PgwireFixture::start(Arc::clone(&context.cassie)));
    let clients = runtime.block_on(fixture.prepared_clients(
        &format!("SELECT id, search_score(body, $1) AS score FROM {TABLE} WHERE search(body, $1) ORDER BY score DESC, id ASC LIMIT 10"),
        4,
    ));
    let ranked_oracle_preflight = runtime.block_on(async {
        let expected = oracle_ids(profile.seeded_rows, TERMS[0]);
        let rows = clients[0]
            .0
            .query(&clients[0].2, &[&TERMS[0]])
            .await
            .expect("full-text oracle preflight query");
        rows.iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>()
            == expected
    });
    let indexed_execution = context.cassie.execute_sql(&context.session,
        &format!("EXPLAIN SELECT id, search_score(body, 'w17') AS score FROM {TABLE} WHERE search(body, 'w17') ORDER BY score DESC, id ASC LIMIT 10"), Vec::new())
        .expect("explain full-text query").rows().iter().any(|row| row.iter().any(|value| value.as_str().is_some_and(|text| text.to_ascii_lowercase().contains("fulltext"))));
    let before = context.cassie.metrics()["search"]
        ["fulltext_maintenance_index_bytes_written_total"]
        .as_u64()
        .unwrap_or_default();
    let cassie = Arc::clone(&context.cassie);
    let collection_for_writer = collection.clone();
    let rows = profile.seeded_rows;
    let rate = 2_000 / profile.rate_scale;
    let writer = thread::spawn(move || {
        product::run_fixed_rate(rate, profile.duration, |sequence| {
            let row = usize::try_from(sequence % u64::try_from(rows).expect("row count fits u64"))
                .expect("document id fits usize")
                + 1;
            cassie.midge.put_document(
            &collection_for_writer,
            Some(format!("doc-{row:08}")),
            serde_json::json!({"id": row, "body": document_body(rows + usize::try_from(sequence).unwrap_or_default())}),
        ).map_err(|error| error.to_string())
        })
    });
    let (query_latency_us, ranked_queries) =
        runtime.block_on(run_readers(clients, profile.duration));
    let writer = writer.join().expect("full-text writer thread");
    let after = context.cassie.metrics()["search"]
        ["fulltext_maintenance_index_bytes_written_total"]
        .as_u64()
        .unwrap_or_default();
    runtime.block_on(fixture.shutdown());
    context.cassie.shutdown();
    WindowReport {
        writer,
        query_latency_us,
        ranked_queries,
        indexed_execution,
        index_bytes_written: after.saturating_sub(before),
        ranked_oracle_preflight,
    }
}

fn seed_documents(context: &workloads::BenchContext, collection: &str, rows: usize) {
    for start in (0..rows).step_by(BATCH) {
        let end = (start + BATCH).min(rows);
        let documents = (start..end)
            .map(|row| {
                (
                    Some(format!("doc-{row:08}")),
                    serde_json::json!({"id": row + 1, "body": document_body(row)}),
                )
            })
            .collect();
        context
            .cassie
            .midge
            .put_documents(collection, documents)
            .expect("seed full-text document batch");
    }
}

fn document_body(row: usize) -> String {
    (0..WORDS)
        .map(|position| {
            format!(
                "w{}",
                (row as u64 * 97 + position as u64 * 193 + (row as u64 >> 4)) % VOCABULARY
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn oracle_ids(rows: usize, term: &str) -> Vec<i32> {
    use cassie::search::{bm25, tokenizer};

    let documents = (0..rows)
        .map(|row| tokenizer::tokenize(&document_body(row)))
        .collect::<Vec<_>>();
    let term = tokenizer::tokenize(term)
        .into_iter()
        .next()
        .expect("query term token");
    let document_frequency = documents
        .iter()
        .filter(|tokens| tokens.contains(&term))
        .count();
    let mut ranked = documents
        .iter()
        .enumerate()
        .filter_map(|(row, tokens)| {
            let frequency = tokens.iter().filter(|token| **token == term).count();
            (frequency > 0).then(|| {
                let score = bm25::bm25_score(
                    cassie::types::numeric::usize_to_f64(frequency),
                    cassie::types::numeric::usize_to_f64(document_frequency),
                    cassie::types::numeric::usize_to_f64(rows),
                    BM25_K1,
                    BM25_B,
                    cassie::types::numeric::usize_to_f64(tokens.len()),
                    cassie::types::numeric::usize_to_f64(WORDS),
                );
                (
                    i32::try_from(row + 1).expect("oracle document id fits i32"),
                    score,
                )
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.into_iter().take(10).map(|(id, _)| id).collect()
}

async fn run_readers(
    clients: Vec<(
        tokio_postgres::Client,
        tokio::task::JoinHandle<()>,
        tokio_postgres::Statement,
    )>,
    duration: Duration,
) -> (Vec<u64>, usize) {
    let stop = Instant::now() + duration;
    let mut readers = JoinSet::new();
    for (reader, (client, _connection, statement)) in clients.into_iter().enumerate() {
        readers.spawn(async move {
            let mut samples = Vec::new();
            let mut ranked = 0;
            let mut request = 0;
            while Instant::now() < stop {
                let term = TERMS[(request + reader) % TERMS.len()];
                let started = Instant::now();
                let rows = client
                    .query(&statement, &[&term])
                    .await
                    .expect("full-text top-10 query");
                let elapsed = started.elapsed();
                assert!(
                    !rows.is_empty() && rows.len() <= 10,
                    "full-text query returns ranked matches"
                );
                let mut previous_score = f64::INFINITY;
                let mut previous_id = i32::MIN;
                for row in &rows {
                    let id: i32 = row.get(0);
                    let score: f64 = row.get(1);
                    assert!(
                        score <= previous_score,
                        "full-text result scores are descending"
                    );
                    if score == previous_score {
                        assert!(id >= previous_id, "ties are ordered by id");
                    }
                    previous_score = score;
                    previous_id = id;
                }
                ranked += 1;
                samples.push(u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX));
                request += 1;
            }
            (samples, ranked)
        });
    }
    let mut samples = Vec::new();
    let mut ranked = 0;
    while let Some(result) = readers.join_next().await {
        let (mut reader_samples, reader_ranked) = result.expect("full-text reader task");
        samples.append(&mut reader_samples);
        ranked += reader_ranked;
    }
    (samples, ranked)
}

fn percentile_us(samples: &[u64], percentile: usize) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted
        .get(
            sorted
                .len()
                .saturating_mul(percentile)
                .div_ceil(100)
                .saturating_sub(1),
        )
        .copied()
        .unwrap_or_default()
}
