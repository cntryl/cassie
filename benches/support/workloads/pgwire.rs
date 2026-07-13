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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio_postgres::NoTls;
use uuid::Uuid;

use super::context::{BenchContext, QueryBreakdownMicros};

pub struct PgwirePreparedBenchContext {
    client: tokio_postgres::Client,
    statement: tokio_postgres::Statement,
    port: u16,
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), CassieError>>,
    connection: tokio::task::JoinHandle<()>,
}

impl Drop for PgwirePreparedBenchContext {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
        self.server.abort();
        self.connection.abort();
    }
}

pub struct PgwireTransportBenchContext {
    client: tokio_postgres::Client,
    port: u16,
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), CassieError>>,
    connection: tokio::task::JoinHandle<()>,
}

impl Drop for PgwireTransportBenchContext {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
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
    let server = spawn_pgwire_server(&ctx).await?;
    let (client, connection) = connect_pgwire_client(server.port).await?;
    let statement = client
        .prepare("SELECT id, title FROM bench_documents WHERE title = $1 ORDER BY id ASC LIMIT 25")
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;

    Ok(PgwirePreparedBenchContext {
        client,
        statement,
        port: server.port,
        shutdown: server.shutdown,
        server: server.server,
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

pub async fn pgwire_transport_context(
    label: &str,
    dataset_rows: usize,
) -> Result<PgwireTransportBenchContext, CassieError> {
    let ctx = super::context::unindexed_context(label, dataset_rows).await?;
    pgwire_transport_for_context(&ctx).await
}

pub async fn pgwire_transport_for_context(
    ctx: &BenchContext,
) -> Result<PgwireTransportBenchContext, CassieError> {
    let server = spawn_pgwire_server(ctx).await?;
    let (client, connection) = connect_pgwire_client(server.port).await?;
    Ok(PgwireTransportBenchContext {
        client,
        port: server.port,
        shutdown: server.shutdown,
        server: server.server,
        connection,
    })
}

pub async fn pgwire_transport_simple_query(ctx: &PgwireTransportBenchContext, sql: &str) -> usize {
    let rows = ctx
        .client
        .simple_query(sql)
        .await
        .expect("execute pgwire simple query")
        .into_iter()
        .filter(|message| matches!(message, tokio_postgres::SimpleQueryMessage::Row(_)))
        .count();
    std::hint::black_box(rows)
}

pub async fn pgwire_transport_binary_query(ctx: &PgwireTransportBenchContext) -> usize {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", ctx.port))
        .await
        .expect("connect binary pgwire benchmark client");
    stream
        .write_all(&binary_startup_frame())
        .await
        .expect("write binary startup");
    let authentication = read_binary_frame(&mut stream).await;
    assert_eq!(
        authentication.0, b'R',
        "binary startup authentication frame"
    );
    loop {
        let frame = read_binary_frame(&mut stream).await;
        if frame.0 == b'Z' {
            assert_eq!(frame.1, vec![b'I'], "binary startup ready state");
            break;
        }
    }

    stream
        .write_all(&binary_parse_frame())
        .await
        .expect("write binary parse");
    stream
        .write_all(&binary_sync_frame())
        .await
        .expect("write binary parse sync");
    stream.flush().await.expect("flush binary parse");
    loop {
        if read_binary_frame(&mut stream).await.0 == b'Z' {
            break;
        }
    }

    stream
        .write_all(&binary_bind_frame())
        .await
        .expect("write binary bind");
    stream
        .write_all(&binary_execute_frame())
        .await
        .expect("write binary execute");
    stream
        .write_all(&binary_sync_frame())
        .await
        .expect("write binary sync");
    stream.flush().await.expect("flush binary query");

    let mut rows = 0usize;
    loop {
        let frame = read_binary_frame(&mut stream).await;
        if frame.0 == b'D' {
            assert_binary_result_row(&frame.1);
            rows = rows.saturating_add(1);
        }
        if frame.0 == b'Z' {
            assert_eq!(frame.1, vec![b'I'], "binary query ready state");
            break;
        }
    }
    std::hint::black_box(rows)
}

pub async fn pgwire_transport_connection_churn(ctx: &PgwireTransportBenchContext) -> usize {
    let (client, connection) = connect_pgwire_client(ctx.port)
        .await
        .expect("connect churn pgwire client");
    let rows = client
        .simple_query("SELECT id FROM bench_documents WHERE score = 1 LIMIT 20")
        .await
        .expect("execute churn pgwire query")
        .into_iter()
        .filter(|message| matches!(message, tokio_postgres::SimpleQueryMessage::Row(_)))
        .count();
    connection.abort();
    std::hint::black_box(rows)
}

pub async fn pgwire_transport_concurrent_connections(
    ctx: &PgwireTransportBenchContext,
    concurrency: usize,
) -> usize {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency.max(1) {
        let port = ctx.port;
        tasks.spawn(async move {
            let (client, connection) = connect_pgwire_client(port)
                .await
                .expect("connect concurrent pgwire client");
            let sql = format!(
                "SELECT id FROM bench_documents WHERE score >= {} LIMIT 20",
                index % 16
            );
            let rows = client
                .simple_query(&sql)
                .await
                .expect("execute concurrent pgwire query")
                .into_iter()
                .filter(|message| matches!(message, tokio_postgres::SimpleQueryMessage::Row(_)))
                .count();
            connection.abort();
            rows
        });
    }

    let mut rows = 0usize;
    while let Some(result) = tasks.join_next().await {
        rows = rows.saturating_add(result.expect("pgwire connection task"));
    }
    std::hint::black_box(rows)
}

pub fn pgwire_prepared_statement_protocol_loop() -> usize {
    let messages = [
        "PARSE stmt|SELECT $1::INT AS value",
        "BIND stmt|7",
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

struct PgwireServerContext {
    port: u16,
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), CassieError>>,
}

async fn spawn_pgwire_server(ctx: &BenchContext) -> Result<PgwireServerContext, CassieError> {
    let addr = reserve_local_addr().map_err(|error| CassieError::Execution(error.to_string()))?;
    let port = addr
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .ok_or_else(|| CassieError::Execution(format!("invalid benchmark address '{addr}'")))?;
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.password.clear();
    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
        addr,
        ctx.cassie.clone(),
        config,
        shutdown.clone(),
    ));
    wait_for_pgwire_server(port).await?;
    Ok(PgwireServerContext {
        port,
        shutdown,
        server,
    })
}

async fn wait_for_pgwire_server(port: u16) -> Result<(), CassieError> {
    let mut last_error = None;
    for _ in 0..100 {
        match connect_pgwire_client(port).await {
            Ok((_, connection)) => {
                connection.abort();
                return Ok(());
            }
            Err(error) => {
                last_error = Some(error.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(CassieError::Execution(format!(
        "pgwire benchmark server did not become ready: {}",
        last_error.unwrap_or_else(|| "no connection attempt completed".to_string())
    )))
}

async fn connect_pgwire_client(
    port: u16,
) -> Result<(tokio_postgres::Client, tokio::task::JoinHandle<()>), CassieError> {
    let mut client_config = tokio_postgres::Config::new();
    client_config.host("127.0.0.1");
    client_config.port(port);
    client_config.user("postgres");
    client_config.dbname("postgres");
    let (client, connection) = client_config.connect(NoTls).await.map_err(|error| {
        CassieError::Execution(format!(
            "{error} (hosts={:?}, ports={:?})",
            client_config.get_hosts(),
            client_config.get_ports()
        ))
    })?;
    let connection = tokio::spawn(async move {
        connection
            .await
            .expect("tokio-postgres connection should stay healthy");
    });
    Ok((client, connection))
}

fn reserve_local_addr() -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    drop(listener);
    Ok(addr.to_string())
}

fn binary_startup_frame() -> Vec<u8> {
    let payload = b"\x00\x03\x00\x00user\0postgres\0database\0postgres\0\0";
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("binary startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

fn binary_parse_frame() -> Vec<u8> {
    let mut payload = b"binary_bench_stmt\0SELECT score, title FROM bench_documents WHERE score = $1 ORDER BY score ASC LIMIT 20\0".to_vec();
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&23_i32.to_be_bytes());
    binary_frontend_frame(b'P', &payload)
}

fn binary_bind_frame() -> Vec<u8> {
    let mut payload = b"binary_bench_portal\0binary_bench_stmt\0".to_vec();
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&4_i32.to_be_bytes());
    payload.extend_from_slice(&1_i32.to_be_bytes());
    payload.extend_from_slice(&2_i16.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&0_i16.to_be_bytes());
    binary_frontend_frame(b'B', &payload)
}

fn binary_execute_frame() -> Vec<u8> {
    let mut payload = b"binary_bench_portal\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    binary_frontend_frame(b'E', &payload)
}

fn binary_sync_frame() -> Vec<u8> {
    binary_frontend_frame(b'S', &[])
}

fn binary_frontend_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(tag);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("binary frontend payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

async fn read_binary_frame(stream: &mut tokio::net::TcpStream) -> (u8, Vec<u8>) {
    let mut tag = [0_u8; 1];
    stream
        .read_exact(&mut tag)
        .await
        .expect("read binary frame tag");
    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .await
        .expect("read binary frame length");
    let payload_length = usize::try_from(i32::from_be_bytes(length) - 4)
        .expect("binary frame length should be non-negative");
    let mut payload = vec![0_u8; payload_length];
    stream
        .read_exact(&mut payload)
        .await
        .expect("read binary frame payload");
    (tag[0], payload)
}

fn assert_binary_result_row(payload: &[u8]) {
    let field_count = i16::from_be_bytes(payload[0..2].try_into().expect("binary field count"));
    assert_eq!(field_count, 2, "binary benchmark result field count");
    let score_length = i32::from_be_bytes(payload[2..6].try_into().expect("binary score length"));
    assert_eq!(score_length, 4, "binary int4 result width");
}
