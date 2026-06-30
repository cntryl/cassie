#![allow(dead_code, unused_imports)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt::Write as _;
use std::future::{ready, Ready};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
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
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::sql::{binder, parameter_count, parameter_type_oids, parse_statement};
use cassie::types::{DataType, FieldSchema, Schema, Value};
use serde_json::json;
use uuid::Uuid;

#[derive(Clone)]
pub struct BenchContext {
    pub cassie: Arc<Cassie>,
    pub session: CassieSession,
    pub collection: String,
    pub(super) _embedding_server: Option<Arc<MockTeiEmbeddingServer>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryBreakdownMicros {
    #[serde(rename = "parse_us")]
    pub parse: u64,
    #[serde(rename = "bind_us")]
    pub bind: u64,
    #[serde(rename = "plan_us")]
    pub plan: u64,
    #[serde(rename = "cache_us")]
    pub cache: u64,
    #[serde(rename = "execute_us")]
    pub execute: u64,
    #[serde(rename = "scan_us")]
    pub scan: u64,
    #[serde(rename = "row_decode_us")]
    pub row_decode: u64,
    #[serde(rename = "filter_us")]
    pub filter: u64,
    #[serde(rename = "projection_us")]
    pub projection: u64,
    #[serde(rename = "sort_us")]
    pub sort: u64,
    #[serde(rename = "result_build_us")]
    pub result_build: u64,
    #[serde(rename = "stats_us")]
    pub stats: u64,
    #[serde(rename = "encode_us")]
    pub encode: u64,
    #[serde(rename = "total_us")]
    pub total: u64,
}

pub struct MockTeiEmbeddingServer {
    base_url: String,
    shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark runtime")
}

pub fn context(label: &str, dataset_rows: usize) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options(
        label,
        dataset_rows,
        BenchIndexOptions::full(),
    ))
}

pub fn scalar_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options(
        label,
        dataset_rows,
        BenchIndexOptions::scalar(),
    ))
}

pub fn column_batch_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(column_batch_context_now(label, dataset_rows))
}

fn column_batch_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    let ctx = context_with_index_options(label, dataset_rows, BenchIndexOptions::none())?;
    let statement = format!(
        "CREATE INDEX {}_column_idx ON {} USING column (title, body, status, score) WITH (segment_size = 256)",
        ctx.collection, ctx.collection
    );
    let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    Ok(ctx)
}

pub fn unindexed_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options(
        label,
        dataset_rows,
        BenchIndexOptions::none(),
    ))
}

pub fn replay_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(replay_context_now(label, dataset_rows))
}

fn replay_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir(dir)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        _embedding_server: None,
    };
    prepare_replay_collection(&ctx, dataset_rows)?;
    Ok(ctx)
}

pub fn time_series_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(time_series_context_now(label, dataset_rows))
}

fn time_series_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir(dir)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_time_series_events".to_string(),
        _embedding_server: None,
    };
    prepare_time_series_collection(&ctx, dataset_rows)?;
    Ok(ctx)
}

pub fn graph_context(label: &str, dataset_rows: usize) -> Ready<Result<BenchContext, CassieError>> {
    ready(graph_context_now(label, dataset_rows))
}

fn graph_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir(dir)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_graph".to_string(),
        _embedding_server: None,
    };
    prepare_graph(&ctx, dataset_rows)?;
    Ok(ctx)
}

#[derive(Debug, Clone, Copy)]
struct BenchIndexOptions {
    include_scalar_indexes: bool,
    include_fulltext_index: bool,
}

impl BenchIndexOptions {
    fn full() -> Self {
        Self {
            include_scalar_indexes: true,
            include_fulltext_index: true,
        }
    }

    fn scalar() -> Self {
        Self {
            include_scalar_indexes: true,
            include_fulltext_index: false,
        }
    }

    fn none() -> Self {
        Self {
            include_scalar_indexes: false,
            include_fulltext_index: false,
        }
    }
}

fn context_with_index_options(
    label: &str,
    dataset_rows: usize,
    index_options: BenchIndexOptions,
) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir(dir)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        _embedding_server: None,
    };
    prepare_collection(&ctx, dataset_rows, index_options)?;
    Ok(ctx)
}

pub fn empty_context(label: &str) -> Ready<Result<BenchContext, CassieError>> {
    ready(empty_context_now(label))
}

fn empty_context_now(label: &str) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir(dir)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    Ok(BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        _embedding_server: None,
    })
}

pub fn context_with_mock_tei_embeddings(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_mock_tei_embeddings_now(label, dataset_rows))
}

fn context_with_mock_tei_embeddings_now(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let server = Arc::new(MockTeiEmbeddingServer::spawn());
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.embeddings = EmbeddingsRuntimeConfig::Tei(SelfHostedEmbeddingRuntimeConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout_seconds: 2,
        max_batch_size: 16,
        max_retries: 1,
    });

    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir, config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        _embedding_server: Some(server),
    };
    prepare_collection(&ctx, dataset_rows, BenchIndexOptions::full())?;
    let statement = format!(
        "CREATE INDEX {}_embedding_idx ON {} USING vector (embedding) WITH (source_field = body, metric = cosine)",
        ctx.collection, ctx.collection
    );
    let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    Ok(ctx)
}

pub(super) fn benchmark_data_dir(label: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-bench-{label}-{}", Uuid::new_v4()));
    if std::env::var("BENCH_MIDGE_DISK").ok().as_deref() == Some("1") {
        return path;
    }

    std::fs::write(&path, b"force benchmark in-memory fallback")
        .expect("write benchmark fallback marker");
    path
}

pub(super) fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).expect("benchmark row index should fit i64")
}

pub(super) fn usize_mod_i64(value: usize, modulus: usize) -> i64 {
    usize_to_i64(value % modulus)
}

pub(super) fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).expect("benchmark row index should fit u64")
}

pub(super) fn usize_to_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).expect("benchmark vector component should fit u16"))
}

pub(super) fn usize_mod_f32(value: usize, modulus: usize) -> f32 {
    usize_to_f32(value % modulus)
}

pub(super) fn u64_to_usize_saturating(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

pub(super) fn duration_divisor(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX).max(1)
}

impl MockTeiEmbeddingServer {
    fn spawn() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock tei server");
        listener
            .set_nonblocking(true)
            .expect("set mock tei nonblocking");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("mock tei server address")
        );
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_thread = shutdown.clone();
        let thread = thread::spawn(move || {
            while !shutdown_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let body = read_http_body(&mut stream);
                        let inputs = serde_json::from_slice::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|value| value["inputs"].as_array().map(std::vec::Vec::len))
                            .unwrap_or(1);
                        let vectors = vec![vec![1.0_f32, 0.0, 0.0]; inputs];
                        let response = serde_json::to_string(&vectors).expect("tei response");
                        let output = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            response.len(),
                            response
                        );
                        let _ = stream.write_all(output.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base_url,
            shutdown,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        self.base_url.clone()
    }
}

impl Drop for MockTeiEmbeddingServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn prepare_collection(
    ctx: &BenchContext,
    dataset_rows: usize,
    index_options: BenchIndexOptions,
) -> Result<(), CassieError> {
    if ctx.cassie.catalog.exists(&ctx.collection) {
        return Ok(());
    }

    let schema = bench_document_schema();
    create_and_register_bench_collection(ctx, &schema)?;
    create_bench_fulltext_index(ctx, index_options)?;
    put_bench_documents(ctx, dataset_rows)?;
    create_bench_scalar_indexes(ctx, index_options)?;
    Ok(())
}

fn bench_document_schema() -> Schema {
    Schema {
        fields: vec![
            FieldSchema {
                name: "id".to_string(),
                data_type: DataType::Text,
                nullable: false,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "status".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(3),
                nullable: true,
            },
        ],
    }
}

fn create_and_register_bench_collection(
    ctx: &BenchContext,
    schema: &Schema,
) -> Result<(), CassieError> {
    ctx.cassie
        .midge
        .create_collection(&ctx.collection, schema.clone())?;
    register_schema(ctx, schema);
    Ok(())
}

fn register_schema(ctx: &BenchContext, schema: &Schema) {
    ctx.cassie.register_collection(
        &ctx.collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
}

fn create_bench_fulltext_index(
    ctx: &BenchContext,
    index_options: BenchIndexOptions,
) -> Result<(), CassieError> {
    if !index_options.include_fulltext_index {
        return Ok(());
    }

    let statement = format!(
        "CREATE INDEX {}_body_idx ON {} USING fulltext (body)",
        ctx.collection, ctx.collection
    );
    let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    Ok(())
}

fn put_bench_documents(ctx: &BenchContext, dataset_rows: usize) -> Result<(), CassieError> {
    let documents = build_bench_documents(dataset_rows);
    if documents.is_empty() {
        return Ok(());
    }

    ctx.cassie
        .midge
        .put_documents(&ctx.collection, documents)
        .map(|_| ())
}

fn build_bench_documents(dataset_rows: usize) -> Vec<(Option<String>, serde_json::Value)> {
    (0..dataset_rows)
        .map(|index| {
            let title = format!("title-{}", index % 16);
            let body = if index % 3 == 0 {
                format!("alpha beta gamma {index}")
            } else {
                format!("delta epsilon {index}")
            };
            let status = if index % 2 == 0 {
                "approved"
            } else {
                "pending"
            };

            (
                Some(format!("doc-{index}")),
                json!({
                    "title": title,
                    "body": body,
                    "score": usize_mod_i64(index, 100),
                    "status": status,
                    "embedding": [
                        usize_mod_f32(index, 7),
                        usize_mod_f32(index, 11),
                        usize_mod_f32(index, 13),
                    ],
                }),
            )
        })
        .collect()
}

fn create_bench_scalar_indexes(
    ctx: &BenchContext,
    index_options: BenchIndexOptions,
) -> Result<(), CassieError> {
    if !index_options.include_scalar_indexes {
        return Ok(());
    }

    let statements = [
        format!(
            "CREATE INDEX {}_title_idx ON {} USING btree (title)",
            ctx.collection, ctx.collection
        ),
        format!(
            "CREATE INDEX {}_score_idx ON {} USING btree (score)",
            ctx.collection, ctx.collection
        ),
        format!(
            "CREATE INDEX {}_status_score_idx ON {} USING btree (status, score)",
            ctx.collection, ctx.collection
        ),
        format!(
            "CREATE INDEX {}_lower_title_idx ON {} USING btree (lower(title))",
            ctx.collection, ctx.collection
        ),
    ];

    for statement in statements {
        let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    }

    Ok(())
}

fn prepare_replay_collection(ctx: &BenchContext, dataset_rows: usize) -> Result<(), CassieError> {
    if ctx.cassie.catalog.exists(&ctx.collection) {
        return Ok(());
    }

    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "id".to_string(),
                data_type: DataType::Text,
                nullable: false,
            },
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "status".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    };

    ctx.cassie
        .midge
        .create_collection(&ctx.collection, schema.clone())?;
    register_schema(ctx, &schema);

    let mut csv = String::new();
    for index in 0..dataset_rows {
        let body = if index % 3 == 0 {
            "alpha beta gamma"
        } else {
            "delta epsilon"
        };
        let status = if index % 2 == 0 {
            "approved"
        } else {
            "pending"
        };
        writeln!(
            csv,
            "doc-{index},title-{},{body},{},{}",
            index % 16,
            index % 100,
            status
        )
        .expect("write benchmark csv row");
    }
    if !csv.is_empty() {
        ctx.cassie.copy_from_csv_stdin(
            &ctx.session,
            &CopyStatement {
                table: ctx.collection.clone(),
                columns: vec![
                    "_id".to_string(),
                    "title".to_string(),
                    "body".to_string(),
                    "score".to_string(),
                    "status".to_string(),
                ],
                format: CopyFormat::Csv,
                header: false,
            },
            csv.as_bytes(),
        )?;
    }

    let statements = [
        format!(
            "CREATE INDEX {}_score_idx ON {} USING btree (score)",
            ctx.collection, ctx.collection
        ),
        format!(
            "CREATE INDEX {}_status_score_idx ON {} USING btree (status, score)",
            ctx.collection, ctx.collection
        ),
    ];

    for statement in statements {
        let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    }

    Ok(())
}

fn prepare_time_series_collection(
    ctx: &BenchContext,
    dataset_rows: usize,
) -> Result<(), CassieError> {
    if ctx.cassie.catalog.exists(&ctx.collection) {
        return Ok(());
    }

    let create = format!(
        "CREATE TABLE {} (tenant TEXT, event_at TIMESTAMP, amount INT, status TEXT)",
        ctx.collection
    );
    ctx.cassie.execute_sql(&ctx.session, &create, vec![])?;

    let statements = [
        format!(
            "CREATE INDEX bench_time_series_time_idx ON {} USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
            ctx.collection
        ),
        format!(
            "CREATE ROLLUP bench_time_series_hourly ON {} USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
            ctx.collection
        ),
        format!(
            "CREATE RETENTION POLICY bench_time_series_retention ON {} USING event_at RETAIN FOR '2 days'",
            ctx.collection
        ),
    ];

    for statement in statements {
        let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    }

    let tenants = ["tenant-a", "tenant-b", "tenant-c", "tenant-d"];
    let mut documents = Vec::with_capacity(dataset_rows);
    for index in 0..dataset_rows {
        let day = 9 + ((index / 24) % 7);
        let hour = index % 24;
        let tenant = tenants[index % tenants.len()];
        documents.push((
            Some(format!("ts-doc-{index}")),
            json!({
                "tenant": tenant,
                "event_at": format!("2026-01-{day:02}T{hour:02}:00:00Z"),
                "amount": usize_mod_i64(index, 100),
                "status": if index % 2 == 0 { "open" } else { "closed" },
            }),
        ));
    }
    if !documents.is_empty() {
        ctx.cassie
            .midge
            .put_fresh_time_series_documents(&ctx.collection, documents)?;
    }

    Ok(())
}

fn prepare_graph(ctx: &BenchContext, dataset_rows: usize) -> Result<(), CassieError> {
    if ctx.cassie.catalog.graph_exists(&ctx.collection) {
        return Ok(());
    }

    let create = format!(
        "CREATE GRAPH {} (NODES (label TEXT), EDGES (source TEXT))",
        ctx.collection
    );
    ctx.cassie.execute_sql(&ctx.session, &create, vec![])?;

    let mut nodes = Vec::with_capacity(dataset_rows);
    for index in 0..dataset_rows {
        nodes.push((
            Some(format!("node-{index}")),
            json!({
                "node_type": "doc",
                "node_id": format!("node-{index}"),
                "label": format!("Node {index}"),
            }),
        ));
    }
    if !nodes.is_empty() {
        ctx.cassie
            .midge
            .put_fresh_graph_documents(&format!("{}_nodes", ctx.collection), nodes)?;
    }

    let mut edges = Vec::with_capacity(dataset_rows.saturating_sub(1));
    for index in 0..dataset_rows.saturating_sub(1) {
        edges.push((
            Some(format!("edge-{index}")),
            json!({
                "edge_id": format!("edge-{index}"),
                "source_type": "doc",
                "source_id": format!("node-{index}"),
                "target_type": "doc",
                "target_id": format!("node-{}", index + 1),
                "edge_type": "links",
                "weight": 1,
                "source": "bench",
            }),
        ));
    }
    if !edges.is_empty() {
        ctx.cassie
            .midge
            .put_fresh_graph_documents(&format!("{}_edges", ctx.collection), edges)?;
    }

    Ok(())
}

fn read_http_body(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut headers_end = 0usize;
    let mut content_length = 0usize;
    while headers_end == 0 {
        let read = stream.read(&mut chunk).expect("read request");
        if read == 0 {
            return Vec::new();
        }

        buffer.extend_from_slice(&chunk[..read]);
        if let Some(separator) = find_request_body_start(&buffer) {
            headers_end = separator;
            content_length = parse_content_length(&buffer);
        }
    }

    while buffer.len() < headers_end.saturating_add(content_length) {
        let read = stream.read(&mut chunk).expect("read request body");
        if read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
    }

    buffer[headers_end..headers_end.saturating_add(content_length)].to_vec()
}

fn find_request_body_start(value: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(value);
    text.find("\r\n\r\n").map(|index| index + 4)
}

fn parse_content_length(value: &[u8]) -> usize {
    let header = String::from_utf8_lossy(value);
    for line in header.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            if let Ok(parsed) = value.trim().parse::<usize>() {
                return parsed;
            }
        }
    }
    0
}
