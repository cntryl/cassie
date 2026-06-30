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

pub fn http_vector_search(ctx: &BenchContext) -> Ready<usize> {
    let body = json!({
        "field": "embedding",
        "query": "[1,0,0]",
        "metric": "cosine",
        "limit": 10,
    });
    let result = search::vector_search(&ctx.cassie, &ctx.collection, body.to_string().as_bytes())
        .expect("vector search");
    let rows = result["rows"].as_array().expect("vector search rows");
    ready(std::hint::black_box(rows.len()))
}

pub fn http_document_get(ctx: &BenchContext) -> Ready<usize> {
    let loaded = documents::get(&ctx.cassie, &ctx.collection, "doc-1").expect("get document");
    std::hint::black_box(loaded);
    ready(1)
}

pub async fn http_concurrent_document_gets(ctx: &BenchContext, concurrency: usize) -> usize {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency.max(1) {
        let cassie = ctx.cassie.clone();
        let collection = ctx.collection.clone();
        tasks.spawn(async move {
            let id = format!("doc-{}", index % 128);
            documents::get(&cassie, &collection, &id).expect("get document");
            1usize
        });
    }

    let mut loaded = 0usize;
    while let Some(result) = tasks.join_next().await {
        loaded += result.expect("document get task");
    }
    std::hint::black_box(loaded)
}

pub fn http_large_result_json(ctx: &BenchContext) -> Ready<usize> {
    let result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "SELECT id, title, body, score FROM bench_documents ORDER BY id LIMIT 512",
            vec![],
        )
        .expect("large result query");
    let encoded = serde_json::to_vec(&result).expect("json encode result");
    ready(std::hint::black_box(encoded.len()))
}

pub fn json_serialization_overhead() -> usize {
    let rows = (0..512)
        .map(|index| {
            json!({
                "id": format!("doc-{index}"),
                "title": format!("title-{}", index % 16),
                "body": "alpha beta gamma",
                "score": index % 100,
            })
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_vec(&rows).expect("json encode rows");
    std::hint::black_box(encoded.len())
}
