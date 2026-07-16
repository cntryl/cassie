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
use tokio::sync::{Mutex, Notify};
use tokio_postgres::NoTls;
use uuid::Uuid;

use super::context::{BenchContext, QueryBreakdownMicros};

pub const PGWIRE_SIMPLE_QUERY: &str =
    "SELECT id, title FROM bench_documents ORDER BY id ASC LIMIT 20";
pub const PGWIRE_EXTENDED_QUERY: &str =
    "SELECT id, title FROM bench_documents WHERE title = $1 ORDER BY id ASC LIMIT 20";
pub const PGWIRE_MULTI_STATEMENT_COMPONENT_QUERY: &str =
    "SELECT id, title FROM bench_documents ORDER BY id ASC LIMIT 10";
const PGWIRE_MULTI_STATEMENT_QUERY: &str = "SELECT id, title FROM bench_documents ORDER BY id ASC LIMIT 10; SELECT id, title FROM bench_documents ORDER BY id ASC LIMIT 10";
pub const PGWIRE_BINARY_QUERY: &str =
    "SELECT score, title FROM bench_documents WHERE score = $1 ORDER BY score ASC LIMIT 20";

pub struct PgwireTransportBenchContext {
    cassie: Arc<Cassie>,
    client: Option<Mutex<tokio_postgres::Client>>,
    extended_statement: tokio_postgres::Statement,
    portal_statement: tokio_postgres::Statement,
    binary_client: Option<Mutex<BinaryPgwireClient>>,
    port: u16,
    shutdown: Arc<Notify>,
    server: Option<tokio::task::JoinHandle<Result<(), CassieError>>>,
    connection: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
struct PgwirePoolClient {
    client: Arc<Mutex<tokio_postgres::Client>>,
    statement: tokio_postgres::Statement,
    score: i32,
}

pub struct PgwireClientPool {
    clients: Vec<PgwirePoolClient>,
    connections: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for PgwireClientPool {
    fn drop(&mut self) {
        for connection in self.connections.drain(..) {
            connection.abort();
        }
    }
}

impl PgwireClientPool {
    #[must_use]
    pub async fn query(&self, client_count: usize) -> usize {
        assert!(client_count > 0, "pgwire client sweep must not be empty");
        assert!(
            client_count <= self.clients.len(),
            "pgwire client sweep exceeds the prepared pool"
        );
        let mut tasks = tokio::task::JoinSet::new();
        for client in self.clients.iter().take(client_count).cloned() {
            tasks.spawn(async move {
                let rows = client
                    .client
                    .lock()
                    .await
                    .query(&client.statement, &[&client.score])
                    .await
                    .expect("execute pooled pgwire query")
                    .len();
                assert_eq!(rows, 20, "pooled pgwire query result cardinality");
                rows
            });
        }

        let mut rows = 0usize;
        while let Some(result) = tasks.join_next().await {
            rows = rows.saturating_add(result.expect("pooled pgwire query task"));
        }
        std::hint::black_box(rows)
    }

    pub async fn shutdown(mut self) {
        self.clients.clear();
        let connections = std::mem::take(&mut self.connections);
        for connection in connections {
            await_pgwire_connection(connection).await;
        }
    }
}

impl Drop for PgwireTransportBenchContext {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
        if let Some(server) = self.server.take() {
            server.abort();
        }
        if let Some(connection) = self.connection.take() {
            connection.abort();
        }
    }
}

impl PgwireTransportBenchContext {
    pub fn cassie(&self) -> Arc<Cassie> {
        self.cassie.clone()
    }

    fn client(&self) -> &Mutex<tokio_postgres::Client> {
        self.client.as_ref().expect("pgwire benchmark client")
    }

    fn binary_client(&self) -> &Mutex<BinaryPgwireClient> {
        self.binary_client
            .as_ref()
            .expect("binary pgwire benchmark client")
    }

    pub async fn shutdown(mut self) -> Result<(), CassieError> {
        if let Some(binary_client) = self.binary_client.take() {
            binary_client.into_inner().shutdown().await?;
        }
        drop(self.client.take());
        self.shutdown.notify_waiters();

        if let Some(mut server) = self.server.take() {
            if let Ok(result) = tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                result.map_err(|error| CassieError::Execution(error.to_string()))??;
            } else {
                server.abort();
                let _ = server.await;
                return Err(CassieError::Execution(
                    "pgwire benchmark server shutdown timed out".to_string(),
                ));
            }
        }
        if let Some(mut connection) = self.connection.take() {
            if tokio::time::timeout(Duration::from_secs(2), &mut connection)
                .await
                .is_err()
            {
                connection.abort();
                let _ = connection.await;
                return Err(CassieError::Execution(
                    "pgwire benchmark client shutdown timed out".to_string(),
                ));
            }
        }
        Ok(())
    }
}

pub fn pgwire_simple_query(ctx: &BenchContext, sql: &str) -> Ready<usize> {
    let messages =
        cassie::pgwire::handlers::query::run_simple_query(&ctx.cassie, &ctx.session, sql, vec![]);
    ready(std::hint::black_box(messages.len()))
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
    let client_config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    let extended_statement = client
        .prepare(PGWIRE_EXTENDED_QUERY)
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let portal_statement = client
        .prepare(PGWIRE_EXTENDED_QUERY)
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let binary_client = BinaryPgwireClient::connect(
        server.port,
        &client_config.user,
        &client_config.database,
        &client_config.password,
    )
    .await?;
    Ok(PgwireTransportBenchContext {
        cassie: ctx.cassie.clone(),
        client: Some(Mutex::new(client)),
        extended_statement,
        portal_statement,
        binary_client: Some(Mutex::new(binary_client)),
        port: server.port,
        shutdown: server.shutdown,
        server: Some(server.server),
        connection: Some(connection),
    })
}

pub async fn pgwire_transport_client_pool(
    ctx: &PgwireTransportBenchContext,
    client_count: usize,
) -> Result<PgwireClientPool, CassieError> {
    assert!(client_count > 0, "pgwire client pool must not be empty");
    let mut clients = Vec::with_capacity(client_count);
    let mut connections = Vec::with_capacity(client_count);
    for index in 0..client_count {
        let (client, connection) = connect_pgwire_client(ctx.port).await?;
        let statement = client
            .prepare("SELECT id FROM bench_documents WHERE score >= $1 LIMIT 20")
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        clients.push(PgwirePoolClient {
            client: Arc::new(Mutex::new(client)),
            statement,
            score: i32::try_from(index % 16).expect("benchmark score should fit i32"),
        });
        connections.push(connection);
    }
    Ok(PgwireClientPool {
        clients,
        connections,
    })
}

pub async fn pgwire_transport_simple_query(ctx: &PgwireTransportBenchContext, sql: &str) -> usize {
    let client = ctx.client().lock().await;
    let rows = client
        .simple_query(sql)
        .await
        .expect("execute pgwire simple query")
        .into_iter()
        .filter(|message| matches!(message, tokio_postgres::SimpleQueryMessage::Row(_)))
        .count();
    std::hint::black_box(rows)
}

pub async fn pgwire_transport_extended_query(ctx: &PgwireTransportBenchContext) -> usize {
    let client = ctx.client().lock().await;
    let rows = client
        .query(&ctx.extended_statement, &[&"title-1"])
        .await
        .expect("execute persistent extended pgwire query");
    assert_eq!(rows.len(), 20, "extended query result cardinality");
    std::hint::black_box(rows.len())
}

pub async fn pgwire_transport_portal_fetch(ctx: &PgwireTransportBenchContext) -> usize {
    let statement = ctx.portal_statement.clone();
    let mut client = ctx.client().lock().await;
    let transaction = client
        .transaction()
        .await
        .expect("begin portal benchmark transaction");
    let portal = transaction
        .bind(&statement, &[&"title-1"])
        .await
        .expect("bind portal benchmark statement");
    let first = transaction
        .query_portal(&portal, 10)
        .await
        .expect("fetch first portal page");
    let second = transaction
        .query_portal(&portal, 10)
        .await
        .expect("fetch second portal page");
    assert_eq!(first.len(), 10, "first portal result cardinality");
    assert_eq!(second.len(), 10, "second portal result cardinality");
    transaction
        .rollback()
        .await
        .expect("rollback portal benchmark transaction");
    std::hint::black_box(2)
}

pub async fn pgwire_transport_cancellation(ctx: &PgwireTransportBenchContext) -> usize {
    let statement = ctx.portal_statement.clone();
    let mut client = ctx.client().lock().await;
    let transaction = client
        .transaction()
        .await
        .expect("begin cancellation benchmark transaction");
    let portal = transaction
        .bind(&statement, &[&"title-1"])
        .await
        .expect("bind cancellation benchmark portal");
    let initial = transaction
        .query_portal(&portal, 1)
        .await
        .expect("suspend cancellation benchmark portal");
    assert_eq!(initial.len(), 1, "cancellation preflight cardinality");
    transaction
        .cancel_token()
        .cancel_query(NoTls)
        .await
        .expect("send pgwire cancellation request");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let error = transaction
        .query_portal(&portal, 1)
        .await
        .expect_err("cancelled portal should fail when resumed");
    assert_eq!(
        error.code().map(tokio_postgres::error::SqlState::code),
        Some("57014"),
        "pgwire cancellation SQLSTATE"
    );
    transaction
        .rollback()
        .await
        .expect("rollback cancellation benchmark transaction");
    std::hint::black_box(1)
}

pub async fn pgwire_transport_multi_statement(ctx: &PgwireTransportBenchContext) -> usize {
    let rows = pgwire_transport_simple_query(ctx, PGWIRE_MULTI_STATEMENT_QUERY).await;
    assert_eq!(rows, 20, "multi-statement result cardinality");
    std::hint::black_box(2)
}

pub async fn pgwire_transport_binary_query(ctx: &PgwireTransportBenchContext) -> usize {
    ctx.binary_client().lock().await.query().await
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
    close_pgwire_client(client, connection).await;
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
            let score = i32::try_from(index % 16).expect("benchmark score should fit i32");
            let rows = client
                .query(
                    "SELECT id FROM bench_documents WHERE score >= $1 LIMIT 20",
                    &[&score],
                )
                .await
                .expect("execute concurrent pgwire query")
                .len();
            close_pgwire_client(client, connection).await;
            rows
        });
    }

    let mut rows = 0usize;
    while let Some(result) = tasks.join_next().await {
        rows = rows.saturating_add(result.expect("pgwire connection task"));
    }
    std::hint::black_box(rows)
}

async fn close_pgwire_client(
    client: tokio_postgres::Client,
    connection: tokio::task::JoinHandle<()>,
) {
    drop(client);
    await_pgwire_connection(connection).await;
}

async fn await_pgwire_connection(mut connection: tokio::task::JoinHandle<()>) {
    tokio::time::timeout(Duration::from_secs(2), &mut connection)
        .await
        .expect("pgwire benchmark client should close before its deadline")
        .expect("pgwire benchmark client connection task");
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
            cassie::pgwire::handlers::query::run_simple_query(
                &cassie,
                &session,
                "SELECT id FROM bench_documents WHERE score >= $1 LIMIT 20",
                vec![Value::Int64(
                    i64::try_from(index % 16).expect("benchmark score should fit i64"),
                )],
            )
            .len()
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
    let runtime_config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    let mut client_config = tokio_postgres::Config::new();
    client_config.host("127.0.0.1");
    client_config.port(port);
    client_config.user(&runtime_config.user);
    client_config.dbname(&runtime_config.database);
    if !runtime_config.password.is_empty() {
        client_config.password(&runtime_config.password);
    }
    let (client, connection) = client_config.connect(NoTls).await.map_err(|error| {
        CassieError::Execution(format!(
            "{error:?} (hosts={:?}, ports={:?})",
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

struct BinaryPgwireClient {
    stream: tokio::net::TcpStream,
}

impl BinaryPgwireClient {
    async fn connect(
        port: u16,
        user: &str,
        database: &str,
        password: &str,
    ) -> Result<Self, CassieError> {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        stream
            .write_all(&binary_startup_frame(user, database))
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        let authentication = read_binary_frame(&mut stream).await;
        assert_eq!(
            authentication.0, b'R',
            "binary startup authentication frame"
        );
        let authentication_code = i32::from_be_bytes(
            authentication.1[0..4]
                .try_into()
                .expect("binary authentication code"),
        );
        if authentication_code == 3 {
            stream
                .write_all(&binary_password_frame(password))
                .await
                .map_err(|error| CassieError::Execution(error.to_string()))?;
            stream
                .flush()
                .await
                .map_err(|error| CassieError::Execution(error.to_string()))?;
        } else {
            assert_eq!(authentication_code, 0, "binary authentication method");
        }
        loop {
            let frame = read_binary_frame(&mut stream).await;
            if frame.0 == b'R' {
                assert_eq!(
                    i32::from_be_bytes(
                        frame.1[0..4]
                            .try_into()
                            .expect("binary authentication success code")
                    ),
                    0,
                    "binary authentication success"
                );
            }
            if frame.0 == b'Z' {
                assert_eq!(frame.1, vec![b'I'], "binary startup ready state");
                break;
            }
        }

        stream
            .write_all(&binary_parse_frame())
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        stream
            .write_all(&binary_sync_frame())
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        stream
            .flush()
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        loop {
            if read_binary_frame(&mut stream).await.0 == b'Z' {
                break;
            }
        }
        Ok(Self { stream })
    }

    async fn query(&mut self) -> usize {
        self.stream
            .write_all(&binary_bind_frame())
            .await
            .expect("write binary bind");
        self.stream
            .write_all(&binary_execute_frame())
            .await
            .expect("write binary execute");
        self.stream
            .write_all(&binary_sync_frame())
            .await
            .expect("write binary sync");
        self.stream.flush().await.expect("flush binary query");

        let mut rows = 0usize;
        loop {
            let frame = read_binary_frame(&mut self.stream).await;
            if frame.0 == b'D' {
                assert_binary_result_row(&frame.1);
                rows = rows.saturating_add(1);
            }
            if frame.0 == b'Z' {
                assert_eq!(frame.1, vec![b'I'], "binary query ready state");
                break;
            }
        }
        assert_eq!(rows, 20, "binary extended result cardinality");
        std::hint::black_box(rows)
    }

    async fn shutdown(mut self) -> Result<(), CassieError> {
        self.stream
            .shutdown()
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))
    }
}

fn binary_startup_frame(user: &str, database: &str) -> Vec<u8> {
    let mut payload = b"\x00\x03\x00\x00user\0".to_vec();
    payload.extend_from_slice(user.as_bytes());
    payload.extend_from_slice(b"\0database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.extend_from_slice(b"\0\0");
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("binary startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn binary_password_frame(password: &str) -> Vec<u8> {
    let mut payload = password.as_bytes().to_vec();
    payload.push(0);
    binary_frontend_frame(b'p', &payload)
}

fn binary_parse_frame() -> Vec<u8> {
    let mut payload = b"binary_bench_stmt\0".to_vec();
    payload.extend_from_slice(PGWIRE_BINARY_QUERY.as_bytes());
    payload.push(0);
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&23_i32.to_be_bytes());
    binary_frontend_frame(b'P', &payload)
}

fn binary_bind_frame() -> Vec<u8> {
    let mut payload = b"\0binary_bench_stmt\0".to_vec();
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
    let mut payload = b"\0".to_vec();
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
