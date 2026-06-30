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
use tokio_postgres::NoTls;
use uuid::Uuid;

use super::context::{BenchContext, QueryBreakdownMicros};

pub struct PgwirePreparedBenchContext {
    client: tokio_postgres::Client,
    statement: tokio_postgres::Statement,
    server: tokio::task::JoinHandle<Result<(), CassieError>>,
    connection: tokio::task::JoinHandle<()>,
}

impl Drop for PgwirePreparedBenchContext {
    fn drop(&mut self) {
        self.server.abort();
        self.connection.abort();
    }
}

pub fn pgwire_simple_query(ctx: &BenchContext, sql: &str) -> Ready<usize> {
    let messages =
        cassie::pgwire::handlers::query::run_simple_query(&ctx.cassie, &ctx.session, sql, vec![]);
    ready(std::hint::black_box(messages.len()))
}

pub async fn pgwire_prepared_context(
    label: &str,
    dataset_rows: usize,
) -> Result<PgwirePreparedBenchContext, CassieError> {
    let ctx = super::context::context(label, dataset_rows).await?;
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.password.clear();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    drop(listener);

    let server = tokio::spawn(cassie::pgwire::server::run(
        addr.to_string(),
        ctx.cassie.clone(),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client_config = tokio_postgres::Config::new();
    client_config.host("127.0.0.1");
    client_config.port(addr.port());
    client_config.user("postgres");
    client_config.dbname("postgres");
    let (client, connection) = client_config
        .connect(NoTls)
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let connection = tokio::spawn(async move {
        connection
            .await
            .expect("tokio-postgres connection should stay healthy");
    });
    let statement = client
        .prepare("SELECT id, title FROM bench_documents WHERE title = $1 ORDER BY id ASC LIMIT 25")
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;

    Ok(PgwirePreparedBenchContext {
        client,
        statement,
        server,
        connection,
    })
}

pub async fn pgwire_prepared_query(ctx: &PgwirePreparedBenchContext) -> usize {
    let rows = ctx
        .client
        .query(&ctx.statement, &[&"title-1"])
        .await
        .expect("execute prepared pgwire query");
    std::hint::black_box(rows.len())
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

pub fn pgwire_large_result_query(ctx: &BenchContext) -> Ready<usize> {
    pgwire_simple_query(
        ctx,
        "SELECT id, title, body, score FROM bench_documents ORDER BY id LIMIT 512",
    )
}

pub fn pgwire_connection_churn(ctx: &BenchContext) -> Ready<usize> {
    let session = ctx.cassie.create_session("benchmark", None);
    let messages = cassie::pgwire::handlers::query::run_simple_query(
        &ctx.cassie,
        &session,
        "SELECT id FROM bench_documents WHERE score = 1 LIMIT 20",
        vec![],
    );
    ready(std::hint::black_box(messages.len()))
}

pub fn pgwire_connection_pooling(ctx: &BenchContext) -> Ready<usize> {
    pgwire_simple_query(
        ctx,
        "SELECT id FROM bench_documents WHERE score = 1 LIMIT 20",
    )
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
