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

pub async fn sql_binding(ctx: &BenchContext) -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog).expect("bind");
    std::hint::black_box(bound);
    1
}

pub async fn logical_planning(ctx: &BenchContext) -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog).expect("bind");
    let plan = logical::plan(&bound).expect("logical plan");
    std::hint::black_box(plan);
    1
}

pub async fn physical_planning(ctx: &BenchContext) -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog).expect("bind");
    let logical = logical::plan(&bound).expect("logical plan");
    let physical = physical::build(logical);
    std::hint::black_box(physical);
    1
}

pub async fn plan_cache_hit(ctx: &BenchContext) -> usize {
    let sql = "SELECT id, title FROM bench_documents WHERE score >= $1 LIMIT 20";
    let params = vec![Value::Int64(10)];
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, params)
        .expect("plan cache hit");
    std::hint::black_box(result.rows.len())
}

pub async fn plan_cache_miss(ctx: &BenchContext, nonce: usize) -> usize {
    let sql = format!(
        "SELECT id, title FROM bench_documents WHERE score >= 10 AND status IN ('approved', 'pending', 'miss-{nonce}') LIMIT 20"
    );
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, &sql, vec![])
        .expect("plan cache miss");
    std::hint::black_box(result.rows.len())
}

pub async fn execute_sql(ctx: &BenchContext, sql: &str) -> usize {
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, vec![])
        .expect("execute sql");
    std::hint::black_box(result.rows.len())
}

pub async fn simple_10k_query_breakdown(ctx: &BenchContext) -> QueryBreakdownMicros {
    let sql = "SELECT id, title FROM bench_documents WHERE title = 'title-1'";
    let params = Vec::new();

    ctx.cassie
        .execute_sql(&ctx.session, sql, params.clone())
        .expect("warm plan cache");

    let total_started = Instant::now();
    let total_result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, params.clone())
        .expect("timed total query");
    let total_query_us = micros(total_started.elapsed());

    let parse_started = Instant::now();
    let parsed = parse_statement(sql).expect("parse statement");
    let parse_us = micros(parse_started.elapsed());

    let bind_started = Instant::now();
    let bound = binder::bind(parsed.clone(), &ctx.cassie.catalog).expect("bind statement");
    let bind_us = micros(bind_started.elapsed());

    let plan_started = Instant::now();
    let logical = logical::plan(&bound).expect("logical plan");
    let physical = physical::build(logical);
    let plan_us = micros(plan_started.elapsed());

    let cache_started = Instant::now();
    let cache_hit = ctx.cassie.plan_cache_hit_for_diagnostics(
        &parsed,
        &params,
        ExecutionMode::SimpleQuery,
        ctx.session.database.clone(),
    );
    let cache_us = micros(cache_started.elapsed());
    assert!(cache_hit, "expected warmed plan cache hit");

    let execute_started = Instant::now();
    let execute_output =
        cassie::executor::run_with_execution_breakdown(ctx.cassie.as_ref(), physical, params)
            .expect("execute physical plan");
    let execute_us = micros(execute_started.elapsed());

    let encode_started = Instant::now();
    let encoded = serde_json::to_vec(&execute_output.result).expect("encode query result");
    let encode_us = micros(encode_started.elapsed());
    let execution_breakdown = execute_output.breakdown;

    std::hint::black_box(total_result.rows.len());
    std::hint::black_box(encoded.len());

    QueryBreakdownMicros {
        parse_us,
        bind_us,
        plan_us,
        cache_us,
        execute_us,
        scan_us: execution_breakdown.scan_us,
        row_decode_us: execution_breakdown.row_decode_us,
        filter_us: execution_breakdown.filter_us,
        projection_us: execution_breakdown.projection_us,
        sort_us: execution_breakdown.sort_us,
        result_build_us: execution_breakdown.result_build_us,
        stats_us: execution_breakdown.stats_us,
        encode_us,
        total_us: total_query_us.saturating_add(encode_us),
    }
}

fn micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}
