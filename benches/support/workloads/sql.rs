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

use super::bound_sql;
use super::context::{BenchContext, QueryBreakdownMicros};

pub fn sql_binding(ctx: &BenchContext) -> Ready<usize> {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog).expect("bind");
    std::hint::black_box(bound);
    ready(1)
}

pub fn logical_planning(ctx: &BenchContext) -> Ready<usize> {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog).expect("bind");
    let plan = logical::plan(&bound).expect("logical plan");
    std::hint::black_box(plan);
    ready(1)
}

pub fn physical_planning(ctx: &BenchContext) -> Ready<usize> {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog).expect("bind");
    let logical = logical::plan(&bound).expect("logical plan");
    let physical = physical::build(logical);
    std::hint::black_box(physical);
    ready(1)
}

pub fn plan_cache_hit(ctx: &BenchContext) -> Ready<usize> {
    let sql = "SELECT id, title FROM bench_documents WHERE score >= $1 LIMIT 20";
    let params = vec![Value::Int64(10)];
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, params)
        .expect("plan cache hit");
    ready(std::hint::black_box(result.rows.len()))
}

pub fn plan_cache_miss(ctx: &BenchContext, nonce: usize) -> Ready<usize> {
    let statement = bound_sql::plan_cache_miss(nonce);
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, &statement.sql, statement.params)
        .expect("plan cache miss");
    ready(std::hint::black_box(result.rows.len()))
}

pub fn execute_sql(ctx: &BenchContext, sql: &str) -> Ready<usize> {
    execute_sql_with_params(ctx, sql, vec![])
}

pub fn execute_sql_with_params(ctx: &BenchContext, sql: &str, params: Vec<Value>) -> Ready<usize> {
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, params)
        .expect("execute sql");
    ready(std::hint::black_box(result.rows.len()))
}

pub fn recursive_cte_query(ctx: &BenchContext, upper_bound: usize) -> Ready<usize> {
    let statement = bound_sql::recursive_cte(upper_bound);
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, &statement.sql, statement.params)
        .expect("execute recursive CTE benchmark");
    let expected_rows = recursive_cte_result_rows(upper_bound);
    assert_eq!(result.rows.len(), expected_rows);
    ready(std::hint::black_box(result.rows.len()))
}

#[must_use]
pub fn recursive_cte_result_rows(upper_bound: usize) -> usize {
    (0..upper_bound)
        .scan(1_usize, |power, _| {
            let current = *power;
            *power = power.saturating_mul(10);
            Some(current)
        })
        .sum()
}

pub fn window_frame_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, cassie::app::CassieError>> {
    ready(window_frame_context_now(label, dataset_rows))
}

fn window_frame_context_now(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, cassie::app::CassieError> {
    let context =
        super::empty_context::empty_context_with_temp_budget(label, dataset_rows).into_inner()?;
    context.cassie.execute_sql(
        &context.session,
        "CREATE TABLE bench_documents (id TEXT, status TEXT, score INT)",
        vec![],
    )?;
    let documents = (0..dataset_rows)
        .map(|index| {
            (
                Some(format!("doc-{index}")),
                serde_json::json!({
                    "id": format!("doc-{index}"),
                    "status": if index % 2 == 0 { "approved" } else { "pending" },
                    "score": i64::try_from(index % 100).unwrap_or(i64::MAX),
                }),
            )
        })
        .collect::<Vec<_>>();
    context
        .cassie
        .midge
        .put_documents(&context.collection, documents)?;
    Ok(context)
}

pub fn window_frame_query(ctx: &BenchContext, expected_rows: usize) -> Ready<usize> {
    let result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "SELECT status, score, first_value(score) OVER (PARTITION BY status ORDER BY score ROWS BETWEEN 3 PRECEDING AND 3 FOLLOWING) AS first_score, last_value(score) OVER (PARTITION BY status ORDER BY score ROWS BETWEEN 3 PRECEDING AND 3 FOLLOWING) AS last_score FROM bench_documents ORDER BY status, score, id",
            vec![],
        )
        .expect("execute window frame benchmark");
    assert_eq!(result.rows.len(), expected_rows);
    assert!(result.rows.iter().all(|row| row.len() == 4));
    ready(std::hint::black_box(result.rows.len()))
}

pub fn simple_10k_query_breakdown(ctx: &BenchContext) -> Ready<QueryBreakdownMicros> {
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
        &ctx.session.search_path(),
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

    ready(QueryBreakdownMicros {
        parse: parse_us,
        bind: bind_us,
        plan: plan_us,
        cache: cache_us,
        execute: execute_us,
        scan: execution_breakdown.scan_us,
        row_decode: execution_breakdown.row_decode_us,
        filter: execution_breakdown.filter_us,
        projection: execution_breakdown.projection_us,
        sort: execution_breakdown.sort_us,
        result_build: execution_breakdown.result_build_us,
        stats: execution_breakdown.stats_us,
        encode: encode_us,
        total: total_query_us.saturating_add(encode_us),
    })
}

fn micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}
