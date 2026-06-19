#![allow(dead_code)]

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
    cassie.startup().await?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        _embedding_server: None,
    };
    prepare_collection(&ctx, dataset_rows)?;
    Ok(ctx)
}

pub async fn empty_context(label: &str) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-bench-{label}-{}", Uuid::new_v4()));

    let cassie = Arc::new(Cassie::new_with_data_dir(dir)?);
    cassie.startup().await?;
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
    prepare_collection(&ctx, dataset_rows)?;
    let statement = format!(
        "CREATE INDEX {}_embedding_idx ON {} USING vector (embedding) WITH (source_field = body, metric = cosine)",
        ctx.collection, ctx.collection
    );
    let _ = ctx
        .cassie
        .execute_sql(&ctx.session, &statement, vec![])
        .await?;
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
    if ctx.cassie.catalog.exists(&ctx.collection).await {
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
        .create_collection(&ctx.collection, schema.clone())
        ?;
    ctx.cassie
        .register_collection(
            &ctx.collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        ;

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
        let _ = ctx
            .cassie
            .execute_sql(&ctx.session, &statement, vec![])
            .await?;
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

        ctx.cassie
            .midge
            .put_document(
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
            )
            ?;
    }

    Ok(())
}

pub fn row_encode_decode() -> usize {
    let encoded = serde_json::to_vec(&json!({"id":"doc-1","title":"alpha"})).expect("encode row");
    let decoded: serde_json::Value = serde_json::from_slice(&encoded).expect("decode row");
    std::hint::black_box(decoded);
    1
}

pub fn key_encode_decode() -> usize {
    let key = format!("__cassie__/schema/{}", Uuid::new_v4());
    let decoded = key.strip_prefix("__cassie__/schema/").expect("key prefix");
    let reencoded = format!("__cassie__/schema/{decoded}");
    std::hint::black_box(reencoded);
    1
}

pub fn field_lookup() -> usize {
    let schema = CollectionSchema {
        collection: "bench".to_string(),
        fields: vec![
            FieldMeta {
                name: "id".to_string(),
                data_type: DataType::Text,
                is_indexed: true,
                boost: Some(1.0),
            },
            FieldMeta {
                name: "title".to_string(),
                data_type: DataType::Text,
                is_indexed: true,
                boost: Some(1.0),
            },
        ],
    };
    std::hint::black_box(schema.field("title").expect("field"));
    1
}

pub fn field_lookup_by_field_id() -> usize {
    let fields = [
        FieldMeta {
            name: "id".to_string(),
            data_type: DataType::Text,
            is_indexed: true,
            boost: Some(1.0),
        },
        FieldMeta {
            name: "title".to_string(),
            data_type: DataType::Text,
            is_indexed: true,
            boost: Some(1.0),
        },
        FieldMeta {
            name: "body".to_string(),
            data_type: DataType::Text,
            is_indexed: true,
            boost: Some(1.0),
        },
        FieldMeta {
            name: "score".to_string(),
            data_type: DataType::Int,
            is_indexed: true,
            boost: None,
        },
    ];
    let field_id = std::hint::black_box(2usize);
    std::hint::black_box(&fields[field_id]);
    1
}

pub fn predicate_evaluation() -> usize {
    let row = json!({"score": 42, "status": "approved"});
    let passes = row["score"].as_i64().unwrap_or_default() >= 40
        && row["status"].as_str() == Some("approved");
    std::hint::black_box(passes as usize)
}

pub fn batch_filter() -> usize {
    let scores = [1_i64, 10, 100, 3, 25, 8, 99, 7];
    let threshold = std::hint::black_box(10_i64);
    let rows = scores
        .iter()
        .filter(|score| std::hint::black_box(**score) >= threshold)
        .count();
    std::hint::black_box(rows)
}

pub fn batch_projection() -> usize {
    let row = json!({"id":"doc-1","title":"alpha","body":"beta"});
    let projected = json!({"title": row["title"].clone()});
    std::hint::black_box(
        projected
            .as_object()
            .map(|fields| fields.len())
            .unwrap_or(0),
    )
}

pub fn value_comparison() -> usize {
    let left = std::hint::black_box(Value::Int64(1));
    let right = std::hint::black_box(Value::Int64(2));
    std::hint::black_box(left.as_i64().unwrap_or_default() < right.as_i64().unwrap_or_default());
    1
}

pub fn top_k_update() -> usize {
    let mut heap = BinaryHeap::new();
    for score in [3, 1, 7, 2, 9, 4] {
        heap.push(Reverse(score));
        if heap.len() > 3 {
            let _ = heap.pop();
        }
    }
    std::hint::black_box(heap.len())
}

pub fn tokenization() -> usize {
    let tokens = tokenizer::tokenize("Alpha beta, gamma and delta");
    std::hint::black_box(tokens.len())
}

pub fn bm25_score() -> usize {
    let score = bm25::bm25_score(
        std::hint::black_box(3.0),
        std::hint::black_box(10.0),
        std::hint::black_box(1000.0),
        std::hint::black_box(1.2),
        std::hint::black_box(0.75),
        std::hint::black_box(120.0),
        std::hint::black_box(100.0),
    );
    std::hint::black_box(score);
    1
}

pub fn cosine_distance() -> usize {
    let distance = cassie::vector::cosine_distance(&[1.0, 0.0, 0.0], &[0.5, 0.5, 0.0]);
    std::hint::black_box(distance);
    1
}

pub fn dot_product() -> usize {
    let score = cassie::vector::dot_score(&[1.0, 2.0, 3.0], &[0.5, 0.5, 0.5]);
    std::hint::black_box(score);
    1
}

pub fn l2_distance() -> usize {
    let distance = cassie::vector::l2_distance(&[1.0, 2.0, 3.0], &[0.5, 0.5, 0.5]);
    std::hint::black_box(distance);
    1
}

pub fn parameter_binding() -> usize {
    let parsed =
        parse_statement("SELECT * FROM bench WHERE id = $1 AND score = $2").expect("parse");
    let count = parameter_count(&parsed);
    let types = parameter_type_oids(&parsed, &[25, 23]);
    std::hint::black_box(types);
    count
}

pub fn sql_lexing() -> usize {
    let sql = std::hint::black_box(
        "SELECT id, title FROM bench_documents WHERE score >= $1 AND status = 'approved' ORDER BY id LIMIT 20",
    );
    let mut tokens = 0usize;
    let mut in_token = false;
    for byte in sql.bytes() {
        let delimiter =
            byte.is_ascii_whitespace() || matches!(byte, b',' | b'(' | b')' | b'=' | b'<' | b'>');
        if delimiter {
            if in_token {
                tokens += 1;
                in_token = false;
            }
        } else {
            in_token = true;
        }
    }
    if in_token {
        tokens += 1;
    }
    std::hint::black_box(tokens)
}

pub fn row_to_pgwire_encoding() -> usize {
    let message = ServerMessage::DataRow(vec!["alpha".to_string(), "1".to_string()]);
    let encoded = cassie::pgwire::protocol::encode(&message);
    std::hint::black_box(encoded.len())
}

pub fn row_to_json_encoding() -> usize {
    let row = json!({"id":"doc-1","title":"alpha","score":1});
    let encoded = serde_json::to_vec(&row).expect("json encode");
    std::hint::black_box(encoded.len())
}

pub fn sql_parsing() -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    std::hint::black_box(parsed);
    1
}

pub async fn sql_binding(ctx: &BenchContext) -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog)
        
        .expect("bind");
    std::hint::black_box(bound);
    1
}

pub async fn logical_planning(ctx: &BenchContext) -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog)
        .await
        .expect("bind");
    let plan = logical::plan(&bound).expect("logical plan");
    std::hint::black_box(plan);
    1
}

pub async fn physical_planning(ctx: &BenchContext) -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    let bound = binder::bind(parsed, &ctx.cassie.catalog)
        .await
        .expect("bind");
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
        .await
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
        .await
        .expect("plan cache miss");
    std::hint::black_box(result.rows.len())
}

pub async fn execute_sql(ctx: &BenchContext, sql: &str) -> usize {
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, vec![])
        .await
        .expect("execute sql");
    std::hint::black_box(result.rows.len())
}

pub async fn pgwire_simple_query(ctx: &BenchContext, sql: &str) -> usize {
    let messages =
        cassie::pgwire::handlers::query::run_simple_query(&ctx.cassie, &ctx.session, sql, vec![])
            .await;
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
    )
    ;
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
            cassie::pgwire::handlers::query::run_simple_query(&cassie, &session, &sql, vec![])
                
                .len()
        });
    }

    let mut messages = 0usize;
    while let Some(result) = tasks.join_next().await {
        messages += result.expect("pgwire connection task");
    }
    std::hint::black_box(messages)
}

pub async fn http_vector_search(ctx: &BenchContext) -> usize {
    let body = json!({
        "field": "embedding",
        "query": "[1,0,0]",
        "metric": "cosine",
        "limit": 10,
    });
    let result = search::vector_search(&ctx.cassie, &ctx.collection, body.to_string().as_bytes())
        .await
        .expect("vector search");
    let rows = result["rows"].as_array().expect("vector search rows");
    std::hint::black_box(rows.len())
}

pub async fn http_document_get(ctx: &BenchContext) -> usize {
    let loaded = documents::get(&ctx.cassie, &ctx.collection, "doc-1")
        .await
        .expect("get document");
    std::hint::black_box(loaded);
    1
}

pub async fn http_concurrent_document_gets(ctx: &BenchContext, concurrency: usize) -> usize {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency.max(1) {
        let cassie = ctx.cassie.clone();
        let collection = ctx.collection.clone();
        tasks.spawn(async move {
            let id = format!("doc-{}", index % 128);
            documents::get(&cassie, &collection, &id)
                .await
                .expect("get document");
            1usize
        });
    }

    let mut loaded = 0usize;
    while let Some(result) = tasks.join_next().await {
        loaded += result.expect("document get task");
    }
    std::hint::black_box(loaded)
}

pub async fn http_large_result_json(ctx: &BenchContext) -> usize {
    let result = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "SELECT id, title, body, score FROM bench_documents ORDER BY id LIMIT 512",
            vec![],
        )
        .await
        .expect("large result query");
    let encoded = serde_json::to_vec(&result).expect("json encode result");
    std::hint::black_box(encoded.len())
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
        .await
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
        .await
        .expect("create document");
    let id = created["id"].as_str().expect("created id");
    let loaded = documents::get(&ctx.cassie, &ctx.collection, id)
        .await
        .expect("get document");
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
                .await
                .expect("create document");
        let id = created["id"].as_str().expect("created id").to_string();
        let loaded = documents::get(&ctx.cassie, &ctx.collection, &id)
            .await
            .expect("get document");
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
                .await
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
        .await
        .expect("create index")
        .command
        .len();
    let dropped = ctx
        .cassie
        .execute_sql(&ctx.session, &drop, vec![])
        .await
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
            .await
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
