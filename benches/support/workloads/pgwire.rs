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

pub async fn pgwire_simple_query(ctx: &BenchContext, sql: &str) -> usize {
    let messages =
        cassie::pgwire::handlers::query::run_simple_query(&ctx.cassie, &ctx.session, sql, vec![]);
    std::hint::black_box(messages.len())
}

pub fn pgwire_prepared_statement_protocol_loop() -> usize {
    let messages = [
        "PARSE stmt|SELECT id FROM bench_documents WHERE score = $1",
        "BIND stmt|42",
        "DESCRIBE stmt",
        "EXECUTE stmt",
        "SYNC",
    ];
    let mut decoded = 0usize;
    for message in messages {
        std::hint::black_box(cassie::pgwire::protocol::decode(message));
        decoded += 1;
    }
    std::hint::black_box(decoded)
}

pub async fn pgwire_large_result_query(ctx: &BenchContext) -> usize {
    pgwire_simple_query(
        ctx,
        "SELECT id, title, body, score FROM bench_documents ORDER BY id LIMIT 512",
    )
    .await
}

pub async fn pgwire_connection_churn(ctx: &BenchContext) -> usize {
    let session = ctx.cassie.create_session("benchmark", None);
    let messages = cassie::pgwire::handlers::query::run_simple_query(
        &ctx.cassie,
        &session,
        "SELECT id FROM bench_documents WHERE score = 1 LIMIT 20",
        vec![],
    );
    std::hint::black_box(messages.len())
}

pub async fn pgwire_connection_pooling(ctx: &BenchContext) -> usize {
    pgwire_simple_query(
        ctx,
        "SELECT id FROM bench_documents WHERE score = 1 LIMIT 20",
    )
    .await
}

pub async fn pgwire_concurrent_connections(ctx: &BenchContext, concurrency: usize) -> usize {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency.max(1) {
        let cassie = ctx.cassie.clone();
        tasks.spawn(async move {
            let session = cassie.create_session("benchmark", None);
            let sql = format!(
                "SELECT id FROM bench_documents WHERE score >= {} LIMIT 20",
                index % 16
            );
            cassie::pgwire::handlers::query::run_simple_query(&cassie, &session, &sql, vec![]).len()
        });
    }

    let mut messages = 0usize;
    while let Some(result) = tasks.join_next().await {
        messages += result.expect("pgwire connection task");
    }
    std::hint::black_box(messages)
}
