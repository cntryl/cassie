#![allow(dead_code, unused_imports)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError, CassieSession};
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

use super::context::{BenchContext, QueryBreakdownMicros};
use super::pgwire::pgwire_simple_query;
use super::sql::execute_sql;

pub async fn protocol_comparison_sql(ctx: &BenchContext) -> usize {
    execute_sql(
        ctx,
        "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20",
    )
    .await
}

pub async fn protocol_comparison_pgwire(ctx: &BenchContext) -> usize {
    pgwire_simple_query(
        ctx,
        "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20",
    )
    .await
}

pub async fn protocol_comparison_http(ctx: &BenchContext) -> usize {
    let result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "SELECT id, title FROM bench_documents WHERE title = 'title-1' LIMIT 20",
            vec![],
        )
        .expect("http comparison query");
    let encoded = serde_json::to_vec(&result).expect("json encode comparison");
    std::hint::black_box(encoded.len())
}

pub async fn http_document_create_get(ctx: &BenchContext) -> usize {
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
    1
}

pub async fn timed_http_document_create_get(ctx: &BenchContext) -> Duration {
    timed_http_document_create_get_batch(ctx, 1).await
}

pub async fn timed_http_document_create_get_batch(
    ctx: &BenchContext,
    batch_size: usize,
) -> Duration {
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
    let elapsed = started.elapsed();
    for id in ids {
        ctx.cassie
            .midge
            .delete_document(&ctx.collection, &id)
            .expect("cleanup document");
    }
    elapsed / batch_size as u32
}

pub async fn ingest_document(ctx: &BenchContext) -> usize {
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

pub async fn mixed_ingest_query(ctx: &BenchContext) -> usize {
    let written = ingest_document(ctx).await;
    let read = execute_sql(
        ctx,
        "SELECT id, title FROM bench_documents WHERE title = 'benchmark-title' LIMIT 20",
    )
    .await;
    std::hint::black_box(written + read)
}

pub async fn concurrent_queries(ctx: &BenchContext, concurrency: usize) -> usize {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency.max(1) {
        let cassie = ctx.cassie.clone();
        let session = ctx.session.clone();
        tasks.spawn(async move {
            let sql = format!(
                "SELECT id, title FROM bench_documents WHERE score >= {} LIMIT 20",
                index % 16
            );
            cassie
                .execute_sql(&session, &sql, vec![])
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

pub async fn projection_rebuild_query(ctx: &BenchContext) -> usize {
    execute_sql(
        ctx,
        "SELECT title, body, score, status FROM bench_documents ORDER BY id LIMIT 512",
    )
    .await
}

pub async fn index_rebuild_ddl(ctx: &BenchContext, nonce: usize) -> usize {
    let name = format!("bench_rebuild_idx_{}", nonce);
    let create = format!(
        "CREATE INDEX {name} ON {} USING btree (status)",
        ctx.collection
    );
    let drop = format!("DROP INDEX {name} ON {}", ctx.collection);
    let created = ctx
        .cassie
        .execute_sql(&ctx.session, &create, vec![])
        .expect("create index")
        .command
        .len();
    let dropped = ctx
        .cassie
        .execute_sql(&ctx.session, &drop, vec![])
        .expect("drop index")
        .command
        .len();
    std::hint::black_box(created + dropped)
}

pub async fn large_result_set_query(ctx: &BenchContext) -> usize {
    execute_sql(
        ctx,
        "SELECT id, title, body, score, status FROM bench_documents ORDER BY id LIMIT 512",
    )
    .await
}

pub async fn ten_million_row_query_shape(ctx: &BenchContext) -> usize {
    execute_sql(
        ctx,
        "SELECT id FROM bench_documents WHERE score >= 10 ORDER BY score DESC LIMIT 100",
    )
    .await
}

pub async fn timed_ingest_document(ctx: &BenchContext) -> Duration {
    timed_ingest_document_batch(ctx, 1).await
}

pub async fn timed_ingest_document_batch(ctx: &BenchContext, batch_size: usize) -> Duration {
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
    let elapsed = started.elapsed();
    for id in ids {
        ctx.cassie
            .midge
            .delete_document(&ctx.collection, &id)
            .expect("cleanup ingested document");
    }
    elapsed / batch_size as u32
}
