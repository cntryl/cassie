use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::{CollectionSchema, FieldMeta};
use crate::executor::batch::{self, BatchRow};
use crate::executor::{filter, projection};
use crate::midge::row_blob::{decode_row, encode_row, RowSchema};
use crate::pgwire::protocol::ServerMessage;
use crate::planner::{logical, physical};
use crate::rest;
use crate::search::{bm25, tokenizer};
use crate::sql::ast::QueryStatement;
use crate::sql::{binder, parameter_count, parameter_type_oids, parse_statement};
use crate::types::{DataType, FieldSchema, Schema, Value};

pub type BenchmarkFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BenchmarkMode {
    #[default]
    InProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    pub workload: String,
    pub dataset: String,
    pub iterations: usize,
    pub warmup: usize,
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    #[serde(default)]
    pub mode: BenchmarkMode,
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("bench-output")
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            workload: "row_encode_decode".to_string(),
            dataset: "tiny".to_string(),
            iterations: 10,
            warmup: 3,
            output_dir: default_output_dir(),
            mode: BenchmarkMode::InProcess,
        }
    }
}

impl BenchmarkConfig {
    pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Self, CassieError> {
        let mut config = Self::default();
        let mut args = args.into_iter();
        let _program = args.next();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--workload" => {
                    config.workload = args.next().ok_or_else(|| {
                        CassieError::Parse("--workload requires a value".to_string())
                    })?;
                }
                "--dataset" => {
                    config.dataset = args.next().ok_or_else(|| {
                        CassieError::Parse("--dataset requires a value".to_string())
                    })?;
                }
                "--iterations" => {
                    let value = args.next().ok_or_else(|| {
                        CassieError::Parse("--iterations requires a value".to_string())
                    })?;
                    config.iterations = value.parse().map_err(|error| {
                        CassieError::Parse(format!("invalid iterations: {error}"))
                    })?;
                }
                "--warmup" => {
                    let value = args.next().ok_or_else(|| {
                        CassieError::Parse("--warmup requires a value".to_string())
                    })?;
                    config.warmup = value
                        .parse()
                        .map_err(|error| CassieError::Parse(format!("invalid warmup: {error}")))?;
                }
                "--output-dir" => {
                    let value = args.next().ok_or_else(|| {
                        CassieError::Parse("--output-dir requires a value".to_string())
                    })?;
                    config.output_dir = PathBuf::from(value);
                }
                "--mode" => {
                    let value = args
                        .next()
                        .ok_or_else(|| CassieError::Parse("--mode requires a value".to_string()))?;
                    config.mode = match value.as_str() {
                        "in-process" | "inprocess" => BenchmarkMode::InProcess,
                        other => {
                            return Err(CassieError::Parse(format!(
                                "unsupported benchmark mode '{other}'"
                            )));
                        }
                    };
                }
                "--list" => {
                    config.workload = "__list__".to_string();
                }
                other => {
                    return Err(CassieError::Parse(format!(
                        "unrecognized benchmark argument '{other}'"
                    )));
                }
            }
        }

        if config.iterations == 0 {
            return Err(CassieError::Parse(
                "iterations must be greater than zero".to_string(),
            ));
        }

        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkRecord {
    pub tier: u8,
    pub name: String,
    pub dataset: String,
    pub rows: usize,
    pub duration_ms: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
    pub throughput: f64,
    pub allocations: u64,
    pub bytes_allocated: u64,
    pub cpu_percent: f64,
    pub memory_mb: f64,
}

#[derive(Clone)]
pub struct BenchmarkContext {
    pub cassie: Arc<Cassie>,
    pub session: CassieSession,
    pub dataset_rows: usize,
    pub collection: String,
}

#[derive(Debug, Clone, Copy)]
pub struct BenchmarkCase {
    pub tier: u8,
    pub name: &'static str,
    pub default_dataset: &'static str,
    pub kind: BenchmarkKind,
}

#[derive(Debug, Clone, Copy)]
pub enum BenchmarkKind {
    RowEncodeDecode,
    KeyEncodeDecode,
    FieldLookup,
    PredicateEvaluation,
    BatchFilter,
    BatchProjection,
    ValueComparison,
    TopKUpdate,
    Tokenization,
    Bm25Score,
    CosineDistance,
    DotProduct,
    L2Distance,
    ParameterBinding,
    RowToPgwireEncoding,
    RowToJsonEncoding,
    SqlParsing,
    SqlBinding,
    LogicalPlanning,
    PhysicalPlanning,
    ExecuteQuery {
        sql: &'static str,
    },
    PgwireQuery {
        sql: &'static str,
    },
    HttpSearch {
        field: &'static str,
        query: &'static str,
    },
    IngestDocument,
}

pub fn available_workloads() -> Vec<&'static str> {
    cases().iter().map(|case| case.name).collect()
}

pub fn cases() -> &'static [BenchmarkCase] {
    static CASES: &[BenchmarkCase] = &[
        BenchmarkCase {
            tier: 1,
            name: "row_encode_decode",
            default_dataset: "tiny",
            kind: BenchmarkKind::RowEncodeDecode,
        },
        BenchmarkCase {
            tier: 1,
            name: "key_encode_decode",
            default_dataset: "tiny",
            kind: BenchmarkKind::KeyEncodeDecode,
        },
        BenchmarkCase {
            tier: 1,
            name: "field_lookup",
            default_dataset: "tiny",
            kind: BenchmarkKind::FieldLookup,
        },
        BenchmarkCase {
            tier: 1,
            name: "predicate_evaluation",
            default_dataset: "tiny",
            kind: BenchmarkKind::PredicateEvaluation,
        },
        BenchmarkCase {
            tier: 1,
            name: "batch_filter",
            default_dataset: "tiny",
            kind: BenchmarkKind::BatchFilter,
        },
        BenchmarkCase {
            tier: 1,
            name: "batch_projection",
            default_dataset: "tiny",
            kind: BenchmarkKind::BatchProjection,
        },
        BenchmarkCase {
            tier: 1,
            name: "value_comparison",
            default_dataset: "tiny",
            kind: BenchmarkKind::ValueComparison,
        },
        BenchmarkCase {
            tier: 1,
            name: "top_k_update",
            default_dataset: "tiny",
            kind: BenchmarkKind::TopKUpdate,
        },
        BenchmarkCase {
            tier: 1,
            name: "tokenization",
            default_dataset: "tiny",
            kind: BenchmarkKind::Tokenization,
        },
        BenchmarkCase {
            tier: 1,
            name: "bm25_score",
            default_dataset: "tiny",
            kind: BenchmarkKind::Bm25Score,
        },
        BenchmarkCase {
            tier: 1,
            name: "cosine_distance",
            default_dataset: "tiny",
            kind: BenchmarkKind::CosineDistance,
        },
        BenchmarkCase {
            tier: 1,
            name: "dot_product",
            default_dataset: "tiny",
            kind: BenchmarkKind::DotProduct,
        },
        BenchmarkCase {
            tier: 1,
            name: "l2_distance",
            default_dataset: "tiny",
            kind: BenchmarkKind::L2Distance,
        },
        BenchmarkCase {
            tier: 1,
            name: "query_parameter_binding",
            default_dataset: "tiny",
            kind: BenchmarkKind::ParameterBinding,
        },
        BenchmarkCase {
            tier: 1,
            name: "row_to_pgwire_encoding",
            default_dataset: "tiny",
            kind: BenchmarkKind::RowToPgwireEncoding,
        },
        BenchmarkCase {
            tier: 1,
            name: "row_to_json_encoding",
            default_dataset: "tiny",
            kind: BenchmarkKind::RowToJsonEncoding,
        },
        BenchmarkCase {
            tier: 2,
            name: "sql_parsing",
            default_dataset: "tiny",
            kind: BenchmarkKind::SqlParsing,
        },
        BenchmarkCase {
            tier: 2,
            name: "sql_binding",
            default_dataset: "tiny",
            kind: BenchmarkKind::SqlBinding,
        },
        BenchmarkCase {
            tier: 2,
            name: "logical_planning",
            default_dataset: "tiny",
            kind: BenchmarkKind::LogicalPlanning,
        },
        BenchmarkCase {
            tier: 2,
            name: "physical_planning",
            default_dataset: "tiny",
            kind: BenchmarkKind::PhysicalPlanning,
        },
        BenchmarkCase {
            tier: 2,
            name: "simple_scan_executor",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, title FROM bench_documents WHERE title = 'title-1'",
            },
        },
        BenchmarkCase {
            tier: 2,
            name: "indexed_filter_executor",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id FROM bench_documents WHERE score = 1",
            },
        },
        BenchmarkCase {
            tier: 2,
            name: "fulltext_search_executor",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha')",
            },
        },
        BenchmarkCase {
            tier: 2,
            name: "vector_bruteforce_executor",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 10",
            },
        },
        BenchmarkCase {
            tier: 2,
            name: "hybrid_executor",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 10",
            },
        },
        BenchmarkCase {
            tier: 2,
            name: "projection_write_path",
            default_dataset: "10k",
            kind: BenchmarkKind::IngestDocument,
        },
        BenchmarkCase {
            tier: 3,
            name: "simple_sql_query",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, title FROM bench_documents WHERE title = 'title-1'",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "indexed_filter_query",
            default_dataset: "1m",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id FROM bench_documents WHERE score = 1",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "range_query",
            default_dataset: "1m",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id FROM bench_documents WHERE score >= 10 LIMIT 100",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "sort_limit_query",
            default_dataset: "1m",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id FROM bench_documents ORDER BY score DESC LIMIT 50",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "fulltext_search_query",
            default_dataset: "1m",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "vector_search_query",
            default_dataset: "1m",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "hybrid_search_query",
            default_dataset: "1m",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 20",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "mixed_ingest_query_load",
            default_dataset: "1m",
            kind: BenchmarkKind::IngestDocument,
        },
        BenchmarkCase {
            tier: 3,
            name: "cold_start",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT count(*) FROM bench_documents",
            },
        },
        BenchmarkCase {
            tier: 3,
            name: "warm_start",
            default_dataset: "10k",
            kind: BenchmarkKind::ExecuteQuery {
                sql: "SELECT count(*) FROM bench_documents",
            },
        },
        BenchmarkCase {
            tier: 4,
            name: "pgwire_simple_query",
            default_dataset: "10k",
            kind: BenchmarkKind::PgwireQuery {
                sql: "SELECT id, title FROM bench_documents WHERE title = 'title-1'",
            },
        },
        BenchmarkCase {
            tier: 4,
            name: "http_vector_search",
            default_dataset: "10k",
            kind: BenchmarkKind::HttpSearch {
                field: "embedding",
                query: "[1,0,0]",
            },
        },
    ];

    CASES
}

pub fn resolve_dataset_rows(dataset: &str) -> usize {
    match dataset {
        "tiny" => 128,
        "10k" | "10k_rows" => 10_000,
        "1m" | "1m_rows" => 1_000_000,
        "10m" | "10m_rows" => 10_000_000,
        other => other.parse().unwrap_or(128),
    }
}

pub async fn run(config: &BenchmarkConfig) -> Result<BenchmarkRecord, CassieError> {
    let case = cases()
        .iter()
        .find(|case| case.name == config.workload)
        .ok_or_else(|| {
            CassieError::NotFound(format!("unknown benchmark workload '{}'", config.workload))
        })?;

    let mut path = config.output_dir.clone();
    if path.as_os_str().is_empty() {
        path = default_output_dir();
    }
    std::fs::create_dir_all(&path).map_err(|error| {
        CassieError::Execution(format!(
            "unable to create benchmark output directory: {error}"
        ))
    })?;

    let ctx = build_context(case, config).await?;
    run_case(case, &ctx, config, Some(path.as_path())).await
}

pub async fn run_case(
    case: &BenchmarkCase,
    ctx: &BenchmarkContext,
    config: &BenchmarkConfig,
    output_dir: Option<&Path>,
) -> Result<BenchmarkRecord, CassieError> {
    prepare_case(case, ctx).await?;

    for _ in 0..config.warmup {
        let _ = execute_case(case, ctx).await?;
    }

    let mut samples = Vec::with_capacity(config.iterations);
    let started = Instant::now();
    let mut total_rows = 0usize;
    for _ in 0..config.iterations {
        let iteration_started = Instant::now();
        let observation = execute_case(case, ctx).await?;
        total_rows += observation.rows;
        samples.push(iteration_started.elapsed());
    }
    let duration = started.elapsed();

    let mut durations_ms = samples
        .into_iter()
        .map(|duration| duration.as_millis() as u64)
        .collect::<Vec<_>>();
    durations_ms.sort_unstable();

    let record = BenchmarkRecord {
        tier: case.tier,
        name: case.name.to_string(),
        dataset: config.dataset.clone(),
        rows: total_rows.max(1),
        duration_ms: duration.as_millis() as u64,
        p50_ms: percentile_ms(&durations_ms, 50),
        p95_ms: percentile_ms(&durations_ms, 95),
        p99_ms: percentile_ms(&durations_ms, 99),
        throughput: throughput(total_rows.max(1), duration),
        allocations: 0,
        bytes_allocated: 0,
        cpu_percent: 0.0,
        memory_mb: 0.0,
    };

    if let Some(output_dir) = output_dir {
        let mut file = output_dir.to_path_buf();
        file.push(format!("{}.json", case.name));
        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        std::fs::write(&file, bytes).map_err(|error| {
            CassieError::Execution(format!("unable to write benchmark output: {error}"))
        })?;
    }

    Ok(record)
}

async fn build_context(
    case: &BenchmarkCase,
    config: &BenchmarkConfig,
) -> Result<BenchmarkContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let dir = benchmark_data_dir(case.name, &config.dataset);
    let cassie = Arc::new(Cassie::new_with_data_dir(&dir)?);
    cassie.startup().await?;
    let session = cassie.create_session("benchmark", None).await;
    Ok(BenchmarkContext {
        cassie,
        session,
        dataset_rows: dataset_rows(&config.dataset),
        collection: format!("bench_{}_{}", case.name, config.dataset),
    })
}

async fn prepare_case(case: &BenchmarkCase, ctx: &BenchmarkContext) -> Result<(), CassieError> {
    match case.kind {
        BenchmarkKind::SqlParsing
        | BenchmarkKind::RowEncodeDecode
        | BenchmarkKind::KeyEncodeDecode
        | BenchmarkKind::FieldLookup
        | BenchmarkKind::PredicateEvaluation
        | BenchmarkKind::BatchFilter
        | BenchmarkKind::BatchProjection
        | BenchmarkKind::ValueComparison
        | BenchmarkKind::TopKUpdate
        | BenchmarkKind::Tokenization
        | BenchmarkKind::Bm25Score
        | BenchmarkKind::CosineDistance
        | BenchmarkKind::DotProduct
        | BenchmarkKind::L2Distance
        | BenchmarkKind::ParameterBinding
        | BenchmarkKind::RowToPgwireEncoding
        | BenchmarkKind::RowToJsonEncoding => Ok(()),
        _ => prepare_benchmark_collection(ctx).await,
    }
}

async fn prepare_benchmark_collection(ctx: &BenchmarkContext) -> Result<(), CassieError> {
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
        .await?;
    ctx.cassie
        .register_collection(
            &ctx.collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;

    let session = ctx.cassie.create_session("benchmark", None).await;
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
        format!(
            "CREATE INDEX {}_embedding_idx ON {} USING vector (embedding) WITH (source_field = body, metric = cosine)",
            ctx.collection, ctx.collection
        ),
    ];
    for statement in statements {
        let _ = ctx.cassie.execute_sql(&session, &statement, vec![]).await?;
    }

    for index in 0..ctx.dataset_rows.min(1024) {
        let id = format!("doc-{index}");
        let title = format!("title-{}", index % 16);
        let body = if index % 3 == 0 {
            format!("alpha beta gamma {index}")
        } else {
            format!("delta epsilon {index}")
        };
        let score = (index % 100) as i64;
        let status = if index % 2 == 0 {
            "approved"
        } else {
            "pending"
        };
        let embedding = vec![(index % 7) as f32, (index % 11) as f32, (index % 13) as f32];

        ctx.cassie
            .midge
            .put_document(
                &ctx.collection,
                Some(id),
                json!({
                    "title": title,
                    "body": body,
                    "score": score,
                    "status": status,
                    "embedding": embedding,
                }),
            )
            .await?;
    }

    Ok(())
}

async fn execute_case(
    case: &BenchmarkCase,
    ctx: &BenchmarkContext,
) -> Result<BenchmarkObservation, CassieError> {
    match case.kind {
        BenchmarkKind::RowEncodeDecode => benchmark_row_encode_decode(),
        BenchmarkKind::KeyEncodeDecode => benchmark_key_encode_decode(),
        BenchmarkKind::FieldLookup => benchmark_field_lookup(),
        BenchmarkKind::PredicateEvaluation => benchmark_predicate_evaluation(),
        BenchmarkKind::BatchFilter => benchmark_batch_filter(),
        BenchmarkKind::BatchProjection => benchmark_batch_projection(),
        BenchmarkKind::ValueComparison => benchmark_value_comparison(),
        BenchmarkKind::TopKUpdate => benchmark_top_k_update(),
        BenchmarkKind::Tokenization => benchmark_tokenization(),
        BenchmarkKind::Bm25Score => benchmark_bm25_score(),
        BenchmarkKind::CosineDistance => benchmark_cosine_distance(),
        BenchmarkKind::DotProduct => benchmark_dot_product(),
        BenchmarkKind::L2Distance => benchmark_l2_distance(),
        BenchmarkKind::ParameterBinding => benchmark_parameter_binding(),
        BenchmarkKind::RowToPgwireEncoding => benchmark_row_to_pgwire_encoding(),
        BenchmarkKind::RowToJsonEncoding => benchmark_row_to_json_encoding(),
        BenchmarkKind::SqlParsing => benchmark_sql_parsing(),
        BenchmarkKind::SqlBinding => benchmark_sql_binding(ctx).await,
        BenchmarkKind::LogicalPlanning => benchmark_logical_planning(ctx).await,
        BenchmarkKind::PhysicalPlanning => benchmark_physical_planning(ctx).await,
        BenchmarkKind::ExecuteQuery { sql } => benchmark_execute_query(ctx, sql).await,
        BenchmarkKind::PgwireQuery { sql } => benchmark_pgwire_query(ctx, sql).await,
        BenchmarkKind::HttpSearch { field, query } => {
            benchmark_http_search(ctx, field, query).await
        }
        BenchmarkKind::IngestDocument => benchmark_ingest_document(ctx).await,
    }
}

#[derive(Debug, Clone, Copy)]
struct BenchmarkObservation {
    rows: usize,
}

fn benchmark_row_encode_decode() -> Result<BenchmarkObservation, CassieError> {
    let schema = RowSchema::from_schema(&Schema {
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
        ],
    });
    let payload = json!({"id":"doc-1","title":"alpha"});
    let encoded = encode_row(&schema, &payload)?;
    let _decoded = decode_row(&schema, &encoded)?;
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_key_encode_decode() -> Result<BenchmarkObservation, CassieError> {
    let key = format!("__cassie__/schema/{}", Uuid::new_v4());
    let decoded = key
        .strip_prefix("__cassie__/schema/")
        .ok_or_else(|| CassieError::Parse("invalid key prefix".to_string()))?;
    let _reencoded = format!("__cassie__/schema/{decoded}");
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_field_lookup() -> Result<BenchmarkObservation, CassieError> {
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
    let _field = schema
        .field("title")
        .ok_or_else(|| CassieError::NotFound("field not found".to_string()))?;
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_predicate_evaluation() -> Result<BenchmarkObservation, CassieError> {
    let row = BatchRow::new(vec![
        ("score".to_string(), Value::Int64(42)),
        ("status".to_string(), Value::String("approved".to_string())),
    ]);
    let rows = vec![row];
    let expr = parse_statement("SELECT 1 FROM bench WHERE score >= 40")
        .map_err(|error| CassieError::Parse(error.0))?;
    let QueryStatement::Select(select) = expr.statement else {
        return Err(CassieError::Parse("expected select".to_string()));
    };
    let filter = select
        .filter
        .ok_or_else(|| CassieError::Parse("missing filter".to_string()))?;
    let filtered = filter::filter_rows(rows, &filter, &[], None, &HashMap::new(), None)?;
    Ok(BenchmarkObservation {
        rows: filtered.len(),
    })
}

fn benchmark_batch_filter() -> Result<BenchmarkObservation, CassieError> {
    let rows = vec![
        BatchRow::new(vec![("score".to_string(), Value::Int64(1))]),
        BatchRow::new(vec![("score".to_string(), Value::Int64(10))]),
        BatchRow::new(vec![("score".to_string(), Value::Int64(100))]),
    ];
    let batches = batch::chunk_rows(rows, 2);
    let parsed = parse_statement("SELECT 1 FROM bench WHERE score >= 10")
        .map_err(|error| CassieError::Parse(error.0))?;
    let QueryStatement::Select(select) = parsed.statement else {
        return Err(CassieError::Parse("expected select".to_string()));
    };
    let filter = select
        .filter
        .ok_or_else(|| CassieError::Parse("missing filter".to_string()))?;
    let filtered = filter::filter_batches(batches, &filter, &[], None, &HashMap::new(), None)?;
    Ok(BenchmarkObservation {
        rows: batch::flatten_batches(filtered).len(),
    })
}

fn benchmark_batch_projection() -> Result<BenchmarkObservation, CassieError> {
    let rows = vec![BatchRow::new(vec![
        ("id".to_string(), Value::String("doc-1".to_string())),
        ("title".to_string(), Value::String("alpha".to_string())),
        ("body".to_string(), Value::String("beta".to_string())),
    ])];
    let projection = vec![crate::sql::ast::SelectItem::Column {
        name: "title".to_string(),
        alias: None,
    }];
    let projected = projection::project_batches(
        batch::chunk_rows(rows, 1),
        &projection,
        &[],
        None,
        &HashMap::new(),
        None,
    )?;
    Ok(BenchmarkObservation {
        rows: batch::flatten_batches(projected).len(),
    })
}

fn benchmark_value_comparison() -> Result<BenchmarkObservation, CassieError> {
    let a = Value::Int64(1);
    let b = Value::Int64(2);
    let _ = a.as_i64().unwrap_or_default() < b.as_i64().unwrap_or_default();
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_top_k_update() -> Result<BenchmarkObservation, CassieError> {
    let mut heap = BinaryHeap::new();
    for score in [3, 1, 7, 2, 9, 4] {
        heap.push(Reverse(score));
        if heap.len() > 3 {
            let _ = heap.pop();
        }
    }
    Ok(BenchmarkObservation { rows: heap.len() })
}

fn benchmark_tokenization() -> Result<BenchmarkObservation, CassieError> {
    let tokens = tokenizer::tokenize("Alpha beta, gamma and delta");
    Ok(BenchmarkObservation { rows: tokens.len() })
}

fn benchmark_bm25_score() -> Result<BenchmarkObservation, CassieError> {
    let score = bm25::bm25_score(3.0, 10.0, 1000.0, 1.2, 0.75, 120.0, 100.0);
    let _ = score;
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_cosine_distance() -> Result<BenchmarkObservation, CassieError> {
    let left = [1.0, 0.0, 0.0];
    let right = [0.5, 0.5, 0.0];
    let _ = crate::vector::cosine_distance(&left, &right);
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_dot_product() -> Result<BenchmarkObservation, CassieError> {
    let left = [1.0, 2.0, 3.0];
    let right = [0.5, 0.5, 0.5];
    let _ = crate::vector::dot_score(&left, &right);
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_l2_distance() -> Result<BenchmarkObservation, CassieError> {
    let left = [1.0, 2.0, 3.0];
    let right = [0.5, 0.5, 0.5];
    let _ = crate::vector::l2_distance(&left, &right);
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_parameter_binding() -> Result<BenchmarkObservation, CassieError> {
    let parsed = parse_statement("SELECT * FROM bench WHERE id = $1 AND score = $2")
        .map_err(|error| CassieError::Parse(error.0))?;
    let count = parameter_count(&parsed);
    let _types = parameter_type_oids(&parsed, &[25, 23]);
    Ok(BenchmarkObservation { rows: count })
}

fn benchmark_row_to_pgwire_encoding() -> Result<BenchmarkObservation, CassieError> {
    let message = ServerMessage::DataRow(vec!["alpha".to_string(), "1".to_string()]);
    let encoded = crate::pgwire::protocol::encode(&message);
    let _ = encoded;
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_row_to_json_encoding() -> Result<BenchmarkObservation, CassieError> {
    let row = json!({"id":"doc-1","title":"alpha","score":1});
    let _ = serde_json::to_vec(&row).map_err(|error| CassieError::Parse(error.to_string()))?;
    Ok(BenchmarkObservation { rows: 1 })
}

fn benchmark_sql_parsing() -> Result<BenchmarkObservation, CassieError> {
    let parsed = parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10")
        .map_err(|error| CassieError::Parse(error.0))?;
    let _ = parsed;
    Ok(BenchmarkObservation { rows: 1 })
}

async fn benchmark_sql_binding(
    ctx: &BenchmarkContext,
) -> Result<BenchmarkObservation, CassieError> {
    let parsed = parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10")
        .map_err(|error| CassieError::Parse(error.0))?;
    let bound = binder::bind(parsed, &ctx.cassie.catalog).await?;
    let _ = bound;
    Ok(BenchmarkObservation { rows: 1 })
}

async fn benchmark_logical_planning(
    ctx: &BenchmarkContext,
) -> Result<BenchmarkObservation, CassieError> {
    let parsed = parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10")
        .map_err(|error| CassieError::Parse(error.0))?;
    let bound = binder::bind(parsed, &ctx.cassie.catalog).await?;
    let plan = logical::plan(&bound)?;
    let _ = plan;
    Ok(BenchmarkObservation { rows: 1 })
}

async fn benchmark_physical_planning(
    ctx: &BenchmarkContext,
) -> Result<BenchmarkObservation, CassieError> {
    let parsed = parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10")
        .map_err(|error| CassieError::Parse(error.0))?;
    let bound = binder::bind(parsed, &ctx.cassie.catalog).await?;
    let logical = logical::plan(&bound)?;
    let physical = physical::build(logical);
    let _ = physical;
    Ok(BenchmarkObservation { rows: 1 })
}

async fn benchmark_execute_query(
    ctx: &BenchmarkContext,
    sql: &'static str,
) -> Result<BenchmarkObservation, CassieError> {
    let result = ctx.cassie.execute_sql(&ctx.session, sql, vec![]).await?;
    Ok(BenchmarkObservation {
        rows: result.rows.len(),
    })
}

async fn benchmark_pgwire_query(
    ctx: &BenchmarkContext,
    sql: &'static str,
) -> Result<BenchmarkObservation, CassieError> {
    let messages =
        crate::pgwire::handlers::query::run_simple_query(&ctx.cassie, &ctx.session, sql, vec![])
            .await;
    Ok(BenchmarkObservation {
        rows: messages.len(),
    })
}

async fn benchmark_http_search(
    ctx: &BenchmarkContext,
    field: &'static str,
    query: &'static str,
) -> Result<BenchmarkObservation, CassieError> {
    let body = json!({
        "field": field,
        "query": query,
        "metric": "cosine",
        "limit": 10,
    });
    let result =
        rest::search::vector_search(&ctx.cassie, &ctx.collection, body.to_string().as_bytes())
            .await?;
    let rows = result.as_array().map(|entries| entries.len()).unwrap_or(0);
    Ok(BenchmarkObservation { rows })
}

async fn benchmark_ingest_document(
    ctx: &BenchmarkContext,
) -> Result<BenchmarkObservation, CassieError> {
    let payload = json!({
        "title": "benchmark-title",
        "body": "alpha beta gamma",
        "score": 42,
        "status": "approved",
        "embedding": [1.0, 0.0, 0.0],
    });
    let _id = ctx.cassie.ingest_document(&ctx.collection, payload).await?;
    Ok(BenchmarkObservation { rows: 1 })
}

fn throughput(rows: usize, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds <= 0.0 {
        0.0
    } else {
        rows as f64 / seconds
    }
}

fn percentile_ms(values: &[u64], percentile: u64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let idx = (((values.len() - 1) as f64) * (percentile as f64 / 100.0)).round() as usize;
    values[idx.min(values.len() - 1)]
}

fn dataset_rows(dataset: &str) -> usize {
    resolve_dataset_rows(dataset)
}

fn benchmark_data_dir(name: &str, dataset: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-bench-{name}-{dataset}-{}", Uuid::new_v4()));
    dir.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_parse_benchmark_config() {
        // Arrange
        let args = vec![
            "cassie-bench".to_string(),
            "--workload".to_string(),
            "row_encode_decode".to_string(),
            "--dataset".to_string(),
            "10k".to_string(),
            "--iterations".to_string(),
            "12".to_string(),
            "--warmup".to_string(),
            "3".to_string(),
            "--output-dir".to_string(),
            "/tmp/bench-output".to_string(),
        ];

        // Act
        let config = BenchmarkConfig::parse_args(args).expect("config should parse");

        // Assert
        assert_eq!(config.workload, "row_encode_decode");
        assert_eq!(config.dataset, "10k");
        assert_eq!(config.iterations, 12);
        assert_eq!(config.warmup, 3);
        assert_eq!(config.output_dir, PathBuf::from("/tmp/bench-output"));
    }

    #[test]
    fn should_serialize_benchmark_record_with_expected_fields() {
        // Arrange
        let record = BenchmarkRecord {
            tier: 1,
            name: "row_encode_decode".to_string(),
            dataset: "tiny".to_string(),
            rows: 1,
            duration_ms: 2,
            p50_ms: 2,
            p95_ms: 2,
            p99_ms: 2,
            throughput: 0.5,
            allocations: 0,
            bytes_allocated: 0,
            cpu_percent: 0.0,
            memory_mb: 0.0,
        };

        // Act
        let json = serde_json::to_value(&record).expect("serialize should succeed");

        // Assert
        assert_eq!(json["tier"], 1);
        assert_eq!(json["name"], "row_encode_decode");
        assert_eq!(json["dataset"], "tiny");
        assert!(json.get("throughput").is_some());
        assert!(json.get("cpu_percent").is_some());
        assert!(json.get("memory_mb").is_some());
    }
}
