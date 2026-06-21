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

#[derive(Clone)]
pub struct BenchContext {
    pub cassie: Arc<Cassie>,
    pub session: CassieSession,
    pub collection: String,
    _embedding_server: Option<Arc<MockTeiEmbeddingServer>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryBreakdownMicros {
    pub parse_us: u64,
    pub bind_us: u64,
    pub plan_us: u64,
    pub cache_us: u64,
    pub execute_us: u64,
    pub scan_us: u64,
    pub row_decode_us: u64,
    pub filter_us: u64,
    pub projection_us: u64,
    pub sort_us: u64,
    pub result_build_us: u64,
    pub stats_us: u64,
    pub encode_us: u64,
    pub total_us: u64,
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

pub async fn context(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-bench-{label}-{}", Uuid::new_v4()));

    let cassie = Arc::new(Cassie::new_with_data_dir(dir)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        _embedding_server: None,
    };
    prepare_collection(&ctx, dataset_rows).await?;
    Ok(ctx)
}

pub async fn empty_context(label: &str) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-bench-{label}-{}", Uuid::new_v4()));

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

pub async fn context_with_mock_tei_embeddings(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let server = Arc::new(MockTeiEmbeddingServer::spawn());
    let mut config = CassieRuntimeConfig::from_env();
    config.embeddings = EmbeddingsRuntimeConfig::Tei(SelfHostedEmbeddingRuntimeConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout_seconds: 2,
        max_batch_size: 16,
        max_retries: 1,
    });

    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-bench-{label}-{}", Uuid::new_v4()));

    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir, config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        _embedding_server: Some(server),
    };
    prepare_collection(&ctx, dataset_rows).await?;
    let statement = format!(
        "CREATE INDEX {}_embedding_idx ON {} USING vector (embedding) WITH (source_field = body, metric = cosine)",
        ctx.collection, ctx.collection
    );
    let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    Ok(ctx)
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
                            .and_then(|value| value["inputs"].as_array().map(|items| items.len()))
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

async fn prepare_collection(ctx: &BenchContext, dataset_rows: usize) -> Result<(), CassieError> {
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
            FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(3),
                nullable: true,
            },
        ],
    };

    ctx.cassie
        .midge
        .create_collection(&ctx.collection, schema.clone())?;
    ctx.cassie.register_collection(
        &ctx.collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );

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
            "CREATE INDEX {}_body_idx ON {} USING fulltext (body)",
            ctx.collection, ctx.collection
        ),
    ];

    for statement in statements {
        let _ = ctx.cassie.execute_sql(&ctx.session, &statement, vec![])?;
    }

    for index in 0..dataset_rows.min(1024) {
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

        ctx.cassie.midge.put_document(
            &ctx.collection,
            Some(format!("doc-{index}")),
            json!({
                "title": title,
                "body": body,
                "score": (index % 100) as i64,
                "status": status,
                "embedding": [
                    (index % 7) as f32,
                    (index % 11) as f32,
                    (index % 13) as f32,
                ],
            }),
        )?;
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
