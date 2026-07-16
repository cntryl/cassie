#![allow(dead_code, unused_imports)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::env;
use std::future::{ready, Ready};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
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
use tokio::sync::Notify;
use uuid::Uuid;

use super::context::{BenchContext, QueryBreakdownMicros};

pub const HTTP_ADMIN_QUERY: &str = "SELECT id, title FROM bench_documents ORDER BY id ASC LIMIT 20";

pub struct HttpBenchContext {
    base_url: String,
    collection: String,
    client: reqwest::Client,
    session_cookie: String,
    shutdown: Arc<Notify>,
    server: Option<tokio::task::JoinHandle<Result<(), CassieError>>>,
}

#[derive(Debug)]
pub struct GeneratedHttpTlsMaterial {
    directory: PathBuf,
    certificate_path: PathBuf,
    key_path: PathBuf,
    cleaned: bool,
}

impl GeneratedHttpTlsMaterial {
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn cleanup(mut self) -> Result<(), CassieError> {
        self.clear_generated_environment();
        if self.directory.exists() {
            std::fs::remove_dir_all(&self.directory)
                .map_err(|error| CassieError::Execution(error.to_string()))?;
        }
        self.cleaned = true;
        Ok(())
    }

    fn clear_generated_environment(&self) {
        remove_env_if_matches("CASSIE_REST_TLS_CERT_FILE", &self.certificate_path);
        remove_env_if_matches("CASSIE_REST_TLS_KEY_FILE", &self.key_path);
    }
}

impl Drop for GeneratedHttpTlsMaterial {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        self.clear_generated_environment();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

pub fn configure_http_tls() -> Result<Option<GeneratedHttpTlsMaterial>, CassieError> {
    if env::var_os("CASSIE_REST_TLS_CERT_FILE").is_some()
        || env::var_os("CASSIE_REST_TLS_KEY_FILE").is_some()
    {
        return Ok(None);
    }
    let directory = env::temp_dir().join(format!(
        "cassie-benchmark-http-tls-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory)
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let certificate_path = directory.join("cert.pem");
    let key_path = directory.join("key.pem");
    let material = GeneratedHttpTlsMaterial {
        directory,
        certificate_path,
        key_path,
        cleaned: false,
    };
    let identity = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    std::fs::write(&material.certificate_path, identity.cert.pem())
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    std::fs::write(&material.key_path, identity.key_pair.serialize_pem())
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    env::set_var(
        "CASSIE_REST_TLS_CERT_FILE",
        path_string(&material.certificate_path),
    );
    env::set_var("CASSIE_REST_TLS_KEY_FILE", path_string(&material.key_path));
    Ok(Some(material))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn remove_env_if_matches(key: &str, generated_path: &Path) {
    if env::var_os(key).as_deref() == Some(generated_path.as_os_str()) {
        env::remove_var(key);
    }
}

impl Drop for HttpBenchContext {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

pub async fn http_transport_context(ctx: &BenchContext) -> Result<HttpBenchContext, CassieError> {
    let addr = reserve_local_addr().map_err(|error| CassieError::Execution(error.to_string()))?;
    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
        addr.clone(),
        ctx.cassie.as_ref().clone(),
        shutdown.clone(),
    ));
    let config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    let secure = ctx.cassie.rest_tls_enabled();
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(secure)
        .build()
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let scheme = if secure { "https" } else { "http" };
    let base_url = format!("{scheme}://{addr}");
    wait_for_http_server(&client, &base_url).await?;
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(secure)
        .build()
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let session_cookie =
        login_http_session(&client, &base_url, &config.user, &config.password).await?;
    verify_authenticated_http_contract(&client, &base_url, &session_cookie).await?;
    Ok(HttpBenchContext {
        base_url,
        collection: ctx.collection.clone(),
        client,
        session_cookie,
        shutdown,
        server: Some(server),
    })
}

impl HttpBenchContext {
    fn authorize(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder.header(reqwest::header::COOKIE, &self.session_cookie)
    }

    pub async fn shutdown(mut self) -> Result<(), CassieError> {
        self.shutdown.notify_waiters();
        let Some(mut server) = self.server.take() else {
            return Ok(());
        };
        if let Ok(result) = tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            result.map_err(|error| CassieError::Execution(error.to_string()))?
        } else {
            server.abort();
            let _ = server.await;
            Err(CassieError::Execution(
                "HTTP benchmark server shutdown timed out".to_string(),
            ))
        }
    }
}

pub fn http_vector_search(ctx: &BenchContext) -> Ready<usize> {
    let body = json!({
        "field": "embedding",
        "query": "[1,0,0]",
        "metric": "cosine",
        "limit": 10,
    });
    let result = search::vector_search(&ctx.cassie, &ctx.collection, body.to_string().as_bytes())
        .expect("vector search");
    ready(std::hint::black_box(result.rows.len()))
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

pub async fn http_transport_vector_search(ctx: &HttpBenchContext) -> usize {
    let body = json!({
        "field": "embedding",
        "query": "[1,0,0]",
        "metric": "cosine",
        "limit": 10,
    });
    let response = ctx
        .authorize(ctx.client.post(format!(
            "{}/api/v1/collections/{}/search",
            ctx.base_url, ctx.collection
        )))
        .json(&body)
        .send()
        .await
        .expect("send vector search request")
        .error_for_status()
        .expect("vector search status")
        .json::<serde_json::Value>()
        .await
        .expect("vector search response");
    let rows = response["rows"].as_array().map_or(0, Vec::len);
    std::hint::black_box(rows)
}

pub async fn http_transport_document_create_get_batch(
    ctx: &HttpBenchContext,
    batch_size: usize,
) -> usize {
    let mut completed = 0usize;
    for _ in 0..batch_size.max(1) {
        completed = completed.saturating_add(http_transport_document_create_get(ctx).await);
    }
    completed
}

pub async fn http_transport_document_create_get(ctx: &HttpBenchContext) -> usize {
    let payload = json!({
        "title": "http-benchmark-title",
        "body": "alpha beta gamma",
        "score": 42,
        "status": "approved",
        "embedding": [1.0, 0.0, 0.0],
    });
    let created = ctx
        .authorize(ctx.client.post(format!(
            "{}/api/v1/collections/{}/documents",
            ctx.base_url, ctx.collection
        )))
        .json(&payload)
        .send()
        .await
        .expect("send create document request")
        .error_for_status()
        .expect("create document status")
        .json::<serde_json::Value>()
        .await
        .expect("create document response");
    let id = created["id"].as_str().expect("created document id");
    let loaded = ctx
        .authorize(ctx.client.get(format!(
            "{}/api/v1/collections/{}/documents/{id}",
            ctx.base_url, ctx.collection
        )))
        .send()
        .await
        .expect("send get document request")
        .error_for_status()
        .expect("get document status")
        .json::<serde_json::Value>()
        .await
        .expect("get document response");
    std::hint::black_box(loaded);
    let deleted = ctx
        .authorize(ctx.client.delete(format!(
            "{}/api/v1/collections/{}/documents/{id}",
            ctx.base_url, ctx.collection
        )))
        .send()
        .await
        .expect("send delete document request")
        .error_for_status()
        .expect("delete document status")
        .json::<serde_json::Value>()
        .await
        .expect("delete document response");
    assert_eq!(
        deleted["deleted"].as_bool(),
        Some(true),
        "benchmark document cleanup"
    );
    std::hint::black_box(3)
}

pub async fn http_transport_query(ctx: &HttpBenchContext) -> usize {
    let response = ctx
        .authorize(
            ctx.client
                .post(format!("{}/api/v1/admin/query/execute", ctx.base_url)),
        )
        .json(&json!({
            "sql": HTTP_ADMIN_QUERY
        }))
        .send()
        .await
        .expect("send HTTP query request")
        .error_for_status()
        .expect("HTTP query status")
        .json::<serde_json::Value>()
        .await
        .expect("HTTP query response");
    assert_eq!(
        response["rows"].as_array().map(Vec::len),
        Some(20),
        "HTTP query result cardinality"
    );
    std::hint::black_box(1)
}

pub async fn http_transport_large_result_set(ctx: &HttpBenchContext) -> usize {
    http_transport_document_get_batch(ctx, 512).await
}

pub async fn http_transport_document_get_batch(ctx: &HttpBenchContext, batch_size: usize) -> usize {
    let mut total = 0usize;
    for index in 0..batch_size.max(1) {
        let id = format!("doc-{index}");
        let loaded = ctx
            .authorize(ctx.client.get(format!(
                "{}/api/v1/collections/{}/documents/{id}",
                ctx.base_url, ctx.collection
            )))
            .send()
            .await
            .expect("send get document request")
            .error_for_status()
            .expect("get document status")
            .json::<serde_json::Value>()
            .await
            .expect("get document response");
        std::hint::black_box(loaded);
        total = total.saturating_add(1);
    }
    total
}

pub async fn http_transport_concurrent_document_gets(
    ctx: &HttpBenchContext,
    concurrency: usize,
) -> usize {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency.max(1) {
        let client = ctx.client.clone();
        let url = format!(
            "{}/api/v1/collections/{}/documents/doc-{}",
            ctx.base_url,
            ctx.collection,
            index % 128
        );
        let session_cookie = ctx.session_cookie.clone();
        tasks.spawn(async move {
            let request = client.get(url);
            let request = request.header(reqwest::header::COOKIE, session_cookie);
            let loaded = request
                .send()
                .await
                .expect("send concurrent get request")
                .error_for_status()
                .expect("concurrent get status")
                .json::<serde_json::Value>()
                .await
                .expect("concurrent get response");
            std::hint::black_box(loaded);
            1usize
        });
    }

    let mut loaded = 0usize;
    while let Some(result) = tasks.join_next().await {
        loaded = loaded.saturating_add(result.expect("document get task"));
    }
    std::hint::black_box(loaded)
}

async fn wait_for_http_server(client: &reqwest::Client, base_url: &str) -> Result<(), CassieError> {
    let health_url = format!("{base_url}/health");
    for _ in 0..100 {
        if client
            .get(&health_url)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(CassieError::Execution(
        "rest benchmark server did not become ready".to_string(),
    ))
}

async fn verify_authenticated_http_contract(
    client: &reqwest::Client,
    base_url: &str,
    session_cookie: &str,
) -> Result<(), CassieError> {
    let session = client
        .get(format!("{base_url}/api/v1/auth/session"))
        .header(reqwest::header::COOKIE, session_cookie)
        .send()
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    if !session.status().is_success() {
        return Err(CassieError::Execution(format!(
            "REST current-session probe returned {}",
            session.status()
        )));
    }

    Ok(())
}

async fn login_http_session(
    client: &reqwest::Client,
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<String, CassieError> {
    for attempt in 0..100 {
        let response = client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&json!({
                "username": username,
                "password": password,
            }))
            .send()
            .await;
        match response {
            Ok(response) => {
                let response = response
                    .error_for_status()
                    .map_err(|error| CassieError::Execution(error.to_string()))?;
                return response
                    .headers()
                    .get("set-cookie")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.split(';').next())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        CassieError::Execution(
                            "REST login did not issue a session cookie".to_string(),
                        )
                    });
            }
            Err(_error) if attempt < 99 => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => {
                return Err(CassieError::Execution(format!(
                    "REST login transport failed after retries: {error:?}; source={:?}",
                    std::error::Error::source(&error)
                )));
            }
        }
    }
    Err(CassieError::Execution(
        "REST login retry budget exhausted".to_string(),
    ))
}

fn reserve_local_addr() -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    drop(listener);
    Ok(addr.to_string())
}
