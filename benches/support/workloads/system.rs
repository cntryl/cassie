#![allow(dead_code, unused_imports)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::future::{ready, Ready};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::{
    Cassie, CassieError, CassieSession, ProjectionReplayBatch, ProjectionReplayEvent,
};
use cassie::catalog::{CollectionSchema, FieldMeta};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
};
use cassie::pgwire::protocol::ServerMessage;
use cassie::planner::{logical, physical};
use cassie::rest::{documents, search};
use cassie::runtime::ExecutionMode;
use cassie::search::{bm25, tokenizer};
use cassie::sql::{binder, parameter_count, parameter_type_oids, parse_statement};
use cassie::types::{DataType, FieldSchema, Schema, Value};
use serde_json::json;
use uuid::Uuid;

use super::bound_sql;
use super::context::{
    duration_divisor, u64_to_usize_saturating, usize_to_i64, usize_to_u64, BenchContext,
    QueryBreakdownMicros,
};
use super::sql::execute_sql;

fn projection_counter_delta(
    after: &serde_json::Value,
    before: &serde_json::Value,
    key: &str,
) -> usize {
    counter_delta(after, before, "projections", key)
}

fn counter_delta(
    after: &serde_json::Value,
    before: &serde_json::Value,
    section: &str,
    key: &str,
) -> usize {
    let delta = after[section][key]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(before[section][key].as_u64().unwrap_or_default());
    u64_to_usize_saturating(delta)
}

pub fn http_document_create_get(ctx: &BenchContext) -> Ready<usize> {
    let payload = json!({
        "title": "http-benchmark-title",
        "body": "alpha beta gamma",
        "score": 42,
        "status": "approved",
        "embedding": [1.0, 0.0, 0.0],
    });
    let created = documents::create(&ctx.cassie, &ctx.collection, payload.to_string().as_bytes())
        .expect("create document");
    let id = created["id"].as_str().expect("created id");
    let loaded = documents::get(&ctx.cassie, &ctx.collection, id).expect("get document");
    std::hint::black_box(loaded);
    ready(1)
}

pub fn timed_http_document_create_get(ctx: &BenchContext) -> Ready<Duration> {
    timed_http_document_create_get_batch(ctx, 1)
}

pub fn timed_http_document_create_get_batch(
    ctx: &BenchContext,
    batch_size: usize,
) -> Ready<Duration> {
    let batch_size = batch_size.max(1);
    let payload = json!({
        "title": "http-benchmark-title",
        "body": "alpha beta gamma",
        "score": 42,
        "status": "approved",
        "embedding": [1.0, 0.0, 0.0],
    });
    let mut ids = Vec::with_capacity(batch_size);
    let started = Instant::now();
    for _ in 0..batch_size {
        let created =
            documents::create(&ctx.cassie, &ctx.collection, payload.to_string().as_bytes())
                .expect("create document");
        let id = created["id"].as_str().expect("created id").to_string();
        let loaded = documents::get(&ctx.cassie, &ctx.collection, &id).expect("get document");
        std::hint::black_box(loaded);
        ids.push(id);
    }
    for id in ids {
        ctx.cassie
            .midge
            .delete_document(&ctx.collection, &id)
            .expect("cleanup document");
    }
    let elapsed = started.elapsed();
    ready(elapsed / duration_divisor(batch_size))
}

pub fn ingest_document(ctx: &BenchContext) -> Ready<usize> {
    ready(ingest_document_now(ctx))
}

fn ingest_document_now(ctx: &BenchContext) -> usize {
    let payload = json!({
        "title": "benchmark-title",
        "body": "alpha beta gamma",
        "score": 42,
        "status": "approved",
        "embedding": [1.0, 0.0, 0.0],
    });
    let id = ctx
        .cassie
        .ingest_document(&ctx.collection, payload)
        .expect("ingest document");
    std::hint::black_box(id);
    1
}

pub fn mixed_ingest_query(ctx: &BenchContext) -> Ready<usize> {
    let written = ingest_document_now(ctx);
    let result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "SELECT id, title FROM bench_documents WHERE title = $1 LIMIT 20",
            vec![Value::String("benchmark-title".to_string())],
        )
        .expect("mixed ingest query");
    ready(std::hint::black_box(written + result.rows.len()))
}

pub async fn concurrent_queries(ctx: &BenchContext, concurrency: usize) -> usize {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency.max(1) {
        let cassie = ctx.cassie.clone();
        let session = ctx.session.clone();
        tasks.spawn(async move {
            cassie
                .execute_sql(
                    &session,
                    "SELECT id, title FROM bench_documents WHERE score >= $1 LIMIT 20",
                    vec![Value::Int64(
                        i64::try_from(index % 16).expect("benchmark score should fit i64"),
                    )],
                )
                .expect("concurrent query")
                .rows
                .len()
        });
    }

    let mut rows = 0usize;
    while let Some(result) = tasks.join_next().await {
        rows += result.expect("query task");
    }
    std::hint::black_box(rows)
}

pub fn projection_rebuild_query(ctx: &BenchContext) -> Ready<usize> {
    execute_sql(
        ctx,
        "SELECT title, body, score, status FROM bench_documents ORDER BY id LIMIT 512",
    )
}

pub fn projection_refresh_workflow(ctx: &BenchContext) -> Ready<usize> {
    ready(projection_refresh_workflow_now(ctx))
}

fn projection_refresh_workflow_now(ctx: &BenchContext) -> usize {
    let before = ctx.cassie.metrics();
    let _ = ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE MATERIALIZED PROJECTION IF NOT EXISTS bench_projection AS SELECT title, score, status FROM bench_documents",
        vec![],
    );
    let command_result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "REFRESH MATERIALIZED PROJECTION bench_projection",
            vec![],
        )
        .expect("refresh projection")
        .command
        .len();
    let after = ctx.cassie.metrics();
    let writes = projection_counter_delta(&after, &before, "write_rebuild_target_puts");
    let flushes = projection_counter_delta(&after, &before, "write_batch_flushes");
    std::hint::black_box(writes + flushes + command_result);
    command_result
}

pub fn projection_rebuild_verification(ctx: &BenchContext) -> Ready<usize> {
    let _ = projection_refresh_workflow_now(ctx);
    let rows = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "VERIFY PROJECTION bench_projection MODE full",
            vec![],
        )
        .expect("verify projection")
        .rows
        .len();
    ready(rows)
}

pub fn projection_version_swap(ctx: &BenchContext, _nonce: usize) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let _ = projection_refresh_workflow_now(ctx);
    ctx.cassie
        .execute_sql(
            &ctx.session,
            "ALTER MATERIALIZED PROJECTION bench_projection BUILD VERSION",
            vec![],
        )
        .expect("build projection version");
    let command_len = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "ALTER MATERIALIZED PROJECTION bench_projection ACTIVATE VERSION v1",
            vec![],
        )
        .expect("activate projection version")
        .command
        .len();
    let after = ctx.cassie.metrics();
    let activations = projection_counter_delta(&after, &before, "write_activation_metadata_writes");
    let swaps = projection_counter_delta(&after, &before, "version_swaps");
    std::hint::black_box(activations + swaps + command_len);
    ready(command_len)
}

pub fn projection_duplicate_replay(ctx: &BenchContext, nonce: usize) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let batch = ProjectionReplayBatch {
        projection: ctx.collection.clone(),
        source_identity: "bench-replay-stream".to_string(),
        batch_id: format!("bench-duplicate-batch-{nonce}"),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: format!("bench-duplicate-event-{nonce}"),
            checkpoint: format!("bench-duplicate-checkpoint-{nonce}"),
            position: Some(usize_to_u64(nonce)),
            document_id: format!("bench-duplicate-doc-{nonce}"),
            payload: Some(json!({
                "id": format!("bench-duplicate-doc-{nonce}"),
                "title": "duplicate-title",
                "body": "alpha beta",
                "score": 1,
                "status": "approved",
            })),
        }],
    };
    let first = ctx
        .cassie
        .replay_projection_batch(batch.clone())
        .expect("first projection replay");
    let second = ctx
        .cassie
        .replay_projection_batch(batch)
        .expect("duplicate projection replay");
    let after = ctx.cassie.metrics();
    let duplicate_checks = projection_counter_delta(&after, &before, "write_duplicate_checks");
    let replay_batches = projection_counter_delta(&after, &before, "replay_batches");
    let event_delta = projection_counter_delta(&after, &before, "replay_events_applied");
    std::hint::black_box(duplicate_checks + replay_batches + event_delta);
    ready(std::hint::black_box(u64_to_usize_saturating(
        first
            .applied_event_count
            .saturating_add(second.skipped_duplicate_count),
    )))
}

pub fn projection_lag_catchup(ctx: &BenchContext, nonce: usize) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let events = (0..64)
        .map(|index| ProjectionReplayEvent {
            event_id: format!("bench-catchup-event-{nonce}-{index}"),
            checkpoint: format!("bench-catchup-checkpoint-{nonce}-{index}"),
            position: Some(usize_to_u64(nonce.saturating_mul(64).saturating_add(index))),
            document_id: format!("bench-catchup-doc-{nonce}-{index}"),
            payload: Some(json!({
                "id": format!("bench-catchup-doc-{nonce}-{index}"),
                "title": format!("catchup-title-{nonce}-{index}"),
                "body": "alpha beta gamma",
                "score": usize_to_i64(index),
                "status": "approved",
            })),
        })
        .collect();
    let batch = ProjectionReplayBatch {
        projection: ctx.collection.clone(),
        source_identity: "bench-replay-stream".to_string(),
        batch_id: format!("bench-catchup-batch-{nonce}"),
        lag: 0,
        events,
    };
    let result = ctx
        .cassie
        .replay_projection_batch(batch)
        .expect("projection lag catchup replay");
    let after = ctx.cassie.metrics();
    let applied = projection_counter_delta(&after, &before, "replay_events_applied");
    let duplicates = projection_counter_delta(&after, &before, "replay_duplicates_skipped");
    let batch_count = projection_counter_delta(&after, &before, "replay_batches");
    std::hint::black_box(applied + duplicates + batch_count);
    ready(std::hint::black_box(u64_to_usize_saturating(
        result
            .applied_event_count
            .saturating_add(result.skipped_duplicate_count),
    )))
}

pub fn index_rebuild_ddl(ctx: &BenchContext, nonce: usize) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let (create, drop) = rebuild_index_statements(nonce);
    let created = ctx
        .cassie
        .execute_sql(&ctx.session, create, vec![])
        .expect("create index")
        .command
        .len();
    let dropped = ctx
        .cassie
        .execute_sql(&ctx.session, drop, vec![])
        .expect("drop index")
        .command
        .len();
    let after = ctx.cassie.metrics();
    let row_puts = projection_counter_delta(&after, &before, "write_rebuild_target_puts");
    std::hint::black_box(row_puts + created + dropped);
    ready(std::hint::black_box(created + dropped))
}

pub fn large_result_set_query(ctx: &BenchContext) -> Ready<usize> {
    execute_sql(
        ctx,
        "SELECT id, title, body, score, status FROM bench_documents ORDER BY id LIMIT 512",
    )
}

pub fn ten_million_row_query_shape(ctx: &BenchContext) -> Ready<usize> {
    execute_sql(
        ctx,
        "SELECT id FROM bench_documents WHERE score >= 10 ORDER BY score DESC LIMIT 100",
    )
}

pub fn time_series_window_scan(ctx: &BenchContext) -> Ready<usize> {
    let statement = bound_sql::time_series_window("2026-01-10T00:00:00Z", "2026-01-12T00:00:00Z");
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, &statement.sql, statement.params)
        .expect("time-series window scan");
    let metrics = ctx.cassie.metrics();
    let buckets = metrics["time_series"]["buckets_scanned"]
        .as_u64()
        .unwrap_or_default();
    let bucket_native_hits = metrics["time_series"]["bucket_native_hits"]
        .as_u64()
        .unwrap_or_default();
    std::hint::black_box(buckets);
    std::hint::black_box(bucket_native_hits);
    ready(std::hint::black_box(result.rows.len()))
}

fn rebuild_index_statements(nonce: usize) -> (&'static str, &'static str) {
    const STATEMENTS: [(&str, &str); 4] = [
        (
            "CREATE INDEX bench_rebuild_idx_0 ON bench_documents USING btree (status)",
            "DROP INDEX bench_rebuild_idx_0 ON bench_documents",
        ),
        (
            "CREATE INDEX bench_rebuild_idx_1 ON bench_documents USING btree (status)",
            "DROP INDEX bench_rebuild_idx_1 ON bench_documents",
        ),
        (
            "CREATE INDEX bench_rebuild_idx_2 ON bench_documents USING btree (status)",
            "DROP INDEX bench_rebuild_idx_2 ON bench_documents",
        ),
        (
            "CREATE INDEX bench_rebuild_idx_3 ON bench_documents USING btree (status)",
            "DROP INDEX bench_rebuild_idx_3 ON bench_documents",
        ),
    ];
    STATEMENTS[nonce % STATEMENTS.len()]
}

pub fn time_series_retention_enforcement(ctx: &BenchContext) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "ENFORCE RETENTION POLICY bench_time_series_retention AT '2026-01-10T00:00:00Z'",
            vec![],
        )
        .expect("time-series retention enforcement");
    let after = ctx.cassie.metrics();
    assert!(
        counter_delta(&after, &before, "retention", "enforcements") > 0,
        "time-series retention must record an enforcement"
    );
    assert_eq!(
        counter_delta(&after, &before, "retention", "errors"),
        0,
        "time-series retention must not record an error"
    );
    assert!(!result.command.is_empty(), "retention command report");
    ready(std::hint::black_box(usize::from(
        !result.command.is_empty(),
    )))
}

pub fn time_series_rollup_refresh(ctx: &BenchContext) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "REFRESH ROLLUP bench_time_series_hourly",
            vec![],
        )
        .expect("time-series rollup refresh");
    let after = ctx.cassie.metrics();
    assert!(
        counter_delta(&after, &before, "rollups", "refreshes") > 0,
        "time-series rollup must record a refresh"
    );
    assert!(!result.command.is_empty(), "rollup command report");
    ready(std::hint::black_box(usize::from(
        !result.command.is_empty(),
    )))
}

pub fn timed_ingest_document(ctx: &BenchContext) -> Ready<Duration> {
    timed_ingest_document_batch(ctx, 1)
}

pub fn timed_ingest_document_batch(ctx: &BenchContext, batch_size: usize) -> Ready<Duration> {
    let batch_size = batch_size.max(1);
    let payload = json!({
        "title": "benchmark-title",
        "body": "alpha beta gamma",
        "score": 42,
        "status": "approved",
        "embedding": [1.0, 0.0, 0.0],
    });
    let mut ids = Vec::with_capacity(batch_size);
    let started = Instant::now();
    for _ in 0..batch_size {
        let id = ctx
            .cassie
            .ingest_document(&ctx.collection, payload.clone())
            .expect("ingest document");
        std::hint::black_box(&id);
        ids.push(id);
    }
    for id in ids {
        ctx.cassie
            .midge
            .delete_document(&ctx.collection, &id)
            .expect("cleanup ingested document");
    }
    let elapsed = started.elapsed();
    ready(elapsed / duration_divisor(batch_size))
}
