#![allow(dead_code, unused_imports)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt::Write as _;
use std::future::{ready, Ready};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::catalog::{canonical_relation_name, CollectionSchema, FieldMeta};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
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

use super::mock_tei::MockTeiEmbeddingServer;

#[derive(Clone)]
pub struct BenchContext {
    pub cassie: Arc<Cassie>,
    pub session: CassieSession,
    pub collection: String,
    pub data_dir: PathBuf,
    pub(super) _embedding_server: Option<Arc<MockTeiEmbeddingServer>>,
}

pub const ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES: usize = 64 * 1024 * 1024;

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

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark runtime")
}

pub(super) fn configure_benchmark_environment() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
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
    query_memory_budget_bytes: usize,
    max_result_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options_and_runtime(
        label,
        dataset_rows,
        BenchIndexOptions::scalar(),
        BenchmarkStorageMode::Default,
        |config| {
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
            config.limits.max_result_rows = max_result_rows;
        },
    ))
}

pub(super) fn scaling_query_context_now(
    label: &str,
    dataset_rows: usize,
    aggregation_workers: usize,
) -> Result<BenchContext, CassieError> {
    scaling_query_context_with_mode_now(
        label,
        dataset_rows,
        aggregation_workers,
        BenchmarkStorageMode::Default,
    )
}

pub(super) fn scaling_query_disk_context_now(
    label: &str,
    dataset_rows: usize,
    aggregation_workers: usize,
) -> Result<BenchContext, CassieError> {
    scaling_query_context_with_mode_now(
        label,
        dataset_rows,
        aggregation_workers,
        BenchmarkStorageMode::Disk,
    )
}

fn scaling_query_context_with_mode_now(
    label: &str,
    dataset_rows: usize,
    aggregation_workers: usize,
    storage_mode: BenchmarkStorageMode,
) -> Result<BenchContext, CassieError> {
    context_with_index_options_and_runtime(
        label,
        dataset_rows,
        BenchIndexOptions::scalar(),
        storage_mode,
        |config| {
            configure_scaling_query_runtime(config, dataset_rows, aggregation_workers);
        },
    )
}

pub(super) fn reopen_scaling_query_context_now(
    data_dir: PathBuf,
    dataset_rows: usize,
    aggregation_workers: usize,
    query_memory_budget_bytes: usize,
    vectorized_join_batch_size: usize,
) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    configure_scaling_query_runtime(&mut config, dataset_rows, aggregation_workers);
    config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
    config.limits.vectorized_join_batch_size = vectorized_join_batch_size;
    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(
        data_dir.clone(),
        config,
    )?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    Ok(BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        data_dir,
        _embedding_server: None,
    })
}

fn configure_scaling_query_runtime(
    config: &mut CassieRuntimeConfig,
    dataset_rows: usize,
    aggregation_workers: usize,
) {
    config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
    config.limits.max_result_rows = dataset_rows.max(111_111);
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 1_024;
    config.limits.operator_switch_join_row_threshold = dataset_rows.saturating_mul(2).max(1);
    config.limits.parallel_aggregation_workers = aggregation_workers;
    config.limits.max_query_workers = aggregation_workers.max(1);
}

pub fn worker_scaling_context(
    label: &str,
    dataset_rows: usize,
    aggregation_workers: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options_and_runtime(
        label,
        dataset_rows,
        BenchIndexOptions::none(),
        BenchmarkStorageMode::Default,
        |config| {
            config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
            config.limits.parallel_aggregation_workers = aggregation_workers;
            config.limits.max_query_workers = aggregation_workers.max(1);
        },
    ))
}

pub fn column_batch_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(column_batch_context_now(label, dataset_rows))
}

fn column_batch_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    let ctx = context_with_index_options_and_runtime(
        label,
        dataset_rows,
        BenchIndexOptions::none(),
        BenchmarkStorageMode::Default,
        |config| {
            config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
        },
    )?;
    let _ = ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE INDEX bench_documents_column_idx ON bench_documents USING column (title, body, status, score) WITH (segment_size = 256)",
        vec![],
    )?;
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

pub fn unindexed_disk_context_with_temp_budget(
    label: &str,
    dataset_rows: usize,
    query_memory_budget_bytes: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options_and_runtime(
        label,
        dataset_rows,
        BenchIndexOptions::none(),
        BenchmarkStorageMode::Disk,
        |config| {
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        },
    ))
}

pub fn disk_context_with_temp_budget(
    label: &str,
    dataset_rows: usize,
    query_memory_budget_bytes: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options_and_runtime(
        label,
        dataset_rows,
        BenchIndexOptions::full(),
        BenchmarkStorageMode::Disk,
        |config| {
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        },
    ))
}

pub fn replay_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(replay_context_now(label, dataset_rows))
}

fn replay_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir(dir.clone())?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        data_dir: dir,
        _embedding_server: None,
    };
    prepare_replay_collection(&ctx, dataset_rows)?;
    Ok(ctx)
}

pub fn time_series_context(
    label: &str,
    dataset_rows: usize,
    query_memory_budget_bytes: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(time_series_context_now(
        label,
        dataset_rows,
        query_memory_budget_bytes,
    ))
}

fn time_series_context_now(
    label: &str,
    dataset_rows: usize,
    query_memory_budget_bytes: usize,
) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let dir = benchmark_data_dir(label);
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
    time_series_context_with_dir_and_config(dataset_rows, dir, config)
}

pub fn time_series_disk_context_with_temp_budget(
    label: &str,
    dataset_rows: usize,
    query_memory_budget_bytes: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    configure_benchmark_environment();
    let dir = benchmark_data_dir_for_mode(label, BenchmarkStorageMode::Disk);
    ready(
        CassieRuntimeConfig::from_env()
            .map_err(|error| CassieError::Configuration(error.to_string()))
            .and_then(|mut config| {
                config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
                time_series_context_with_dir_and_config(dataset_rows, dir, config)
            }),
    )
}

fn time_series_context_with_dir_and_config(
    dataset_rows: usize,
    dir: PathBuf,
    config: CassieRuntimeConfig,
) -> Result<BenchContext, CassieError> {
    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir.clone(), config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_time_series_events".to_string(),
        data_dir: dir,
        _embedding_server: None,
    };
    prepare_time_series_collection(&ctx, dataset_rows)?;
    Ok(ctx)
}

pub fn graph_context(label: &str, dataset_rows: usize) -> Ready<Result<BenchContext, CassieError>> {
    ready(graph_context_now(label, dataset_rows))
}

fn graph_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let dir = benchmark_data_dir(label);

    let cassie = Arc::new(Cassie::new_with_data_dir(dir.clone())?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_graph".to_string(),
        data_dir: dir,
        _embedding_server: None,
    };
    prepare_graph(&ctx, dataset_rows)?;
    Ok(ctx)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BenchIndexOptions {
    include_scalar_indexes: bool,
    include_fulltext_index: bool,
}

impl BenchIndexOptions {
    pub(super) fn full() -> Self {
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
    context_with_index_options_and_runtime(
        label,
        dataset_rows,
        index_options,
        BenchmarkStorageMode::Default,
        |_| {},
    )
}

pub fn execution_result_cache_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_index_options_and_runtime(
        label,
        dataset_rows,
        BenchIndexOptions::full(),
        BenchmarkStorageMode::Default,
        |config| {
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::enabled();
        },
    ))
}

#[derive(Debug, Clone, Copy)]
pub(super) enum BenchmarkStorageMode {
    Default,
    Disk,
}

fn context_with_index_options_and_runtime(
    label: &str,
    dataset_rows: usize,
    index_options: BenchIndexOptions,
    storage_mode: BenchmarkStorageMode,
    configure: impl FnOnce(&mut CassieRuntimeConfig),
) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let dir = benchmark_data_dir_for_mode(label, storage_mode);

    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    configure(&mut config);

    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir.clone(), config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        data_dir: dir,
        _embedding_server: None,
    };
    prepare_collection(&ctx, dataset_rows, index_options)?;
    Ok(ctx)
}

pub fn recursive_cte_context(
    label: &str,
    recursion_depth: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(recursive_cte_context_now(label, recursion_depth))
}

fn recursive_cte_context_now(
    label: &str,
    recursion_depth: usize,
) -> Result<BenchContext, CassieError> {
    let expected_rows = recursive_cte_expected_rows(recursion_depth);
    let context = context_with_index_options_and_runtime(
        label,
        0,
        BenchIndexOptions::none(),
        BenchmarkStorageMode::Default,
        |config| {
            config.limits.query_timeout_ms = 0;
            config.limits.cte_recursion_depth = recursion_depth;
            config.limits.max_result_rows = expected_rows;
            config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
        },
    )?;
    context.cassie.execute_sql(
        &context.session,
        "CREATE TABLE recursive_cte_fanout (n INT)",
        vec![],
    )?;
    for _ in 0..10 {
        context.cassie.execute_sql(
            &context.session,
            "INSERT INTO recursive_cte_fanout (n) VALUES (1)",
            vec![],
        )?;
    }
    Ok(context)
}

fn recursive_cte_expected_rows(recursion_depth: usize) -> usize {
    (0..recursion_depth)
        .scan(1_usize, |power, _| {
            let current = *power;
            *power = power.saturating_mul(10);
            Some(current)
        })
        .sum()
}

pub(super) fn benchmark_data_dir(label: &str) -> PathBuf {
    benchmark_data_dir_for_mode(label, BenchmarkStorageMode::Default)
}

pub(super) fn benchmark_data_dir_for_mode(
    label: &str,
    storage_mode: BenchmarkStorageMode,
) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-bench-{label}-{}", Uuid::new_v4()));
    if matches!(storage_mode, BenchmarkStorageMode::Disk)
        || std::env::var("BENCH_MIDGE_DISK").ok().as_deref() == Some("1")
    {
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

pub(super) fn prepare_collection(
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
        canonical_relation_name("postgres", "public", &ctx.collection),
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

    let _ = ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE INDEX bench_documents_body_idx ON bench_documents USING fulltext (body)",
        vec![],
    )?;
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
            let raw_embedding = [
                usize_mod_f32(index, 7),
                usize_mod_f32(index, 11),
                usize_mod_f32(index, 13),
            ];
            let magnitude = raw_embedding
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt();
            let embedding = if magnitude == 0.0 {
                vec![1.0, 0.0, 0.0]
            } else {
                raw_embedding
                    .iter()
                    .map(|value| value / magnitude)
                    .collect::<Vec<_>>()
            };

            (
                Some(format!("doc-{index}")),
                json!({
                    "title": title,
                    "body": body,
                    "score": usize_mod_i64(index, 100),
                    "status": status,
                    "embedding": embedding,
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
        "CREATE INDEX bench_documents_title_idx ON bench_documents USING btree (title)",
        "CREATE INDEX bench_documents_score_idx ON bench_documents USING btree (score)",
        "CREATE INDEX bench_documents_status_score_idx ON bench_documents USING btree (status, score)",
        "CREATE INDEX bench_documents_lower_title_idx ON bench_documents USING btree (lower(title))",
    ];

    for statement in statements {
        let _ = ctx.cassie.execute_sql(&ctx.session, statement, vec![])?;
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
        "CREATE INDEX bench_documents_score_idx ON bench_documents USING btree (score)",
        "CREATE INDEX bench_documents_status_score_idx ON bench_documents USING btree (status, score)",
    ];

    for statement in statements {
        let _ = ctx.cassie.execute_sql(&ctx.session, statement, vec![])?;
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

    ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE GRAPH bench_documents (NODES (label TEXT), EDGES (source TEXT))",
        vec![],
    )?;

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
