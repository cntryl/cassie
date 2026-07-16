use std::collections::VecDeque;
use std::future::{ready, Ready};

use cassie::app::{Cassie, CassieError, ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::catalog::ProjectionMeta;
use cassie::types::{Value, Vector};
use serde_json::json;

use super::context::BenchContext;

pub const RELATIONAL_SCALING_SQL: &str =
    "SELECT id, title FROM bench_documents WHERE score >= $1 ORDER BY score, id LIMIT 25";
pub const JOIN_SCALING_SQL: &str = "SELECT bench_join_users.name, bench_join_orders.total FROM bench_join_users JOIN bench_join_orders ON bench_join_users.user_key = bench_join_orders.order_user_key LIMIT 50";
pub const COLUMN_SCALING_SQL: &str = "SELECT COUNT(*) AS rows, SUM(score) AS score_sum, AVG(score) AS score_avg FROM bench_documents";
pub const WORKER_SCALING_SQL: &str = "SELECT status, COUNT(*) AS total, SUM(score) AS score_sum FROM bench_documents GROUP BY status ORDER BY status";
pub const FULLTEXT_SCALING_SQL: &str = "SELECT id, search_score(body, $1) AS score FROM bench_documents WHERE search(body, $1) ORDER BY score DESC LIMIT 20";
pub const VECTOR_SCALING_SQL: &str = "SELECT id, vector_distance(embedding, $1) AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20";
pub const HYBRID_SCALING_SQL: &str = "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM bench_documents ORDER BY score DESC LIMIT 20";
pub const PROJECTION_REPLAY_EVENTS_PER_BATCH: usize = 64;
pub const PREPARED_PROJECTION_REPLAY_BATCH_COUNT: usize = 4_096;

/// Bounded replay inputs built before fixed-duration measurement begins.
pub struct PreparedProjectionReplayBatches {
    batches: VecDeque<ProjectionReplayBatch>,
}

impl PreparedProjectionReplayBatches {
    /// Removes the next fully prepared batch from the timed-input queue.
    ///
    /// # Panics
    ///
    /// Panics when a fixed-duration run consumes the bounded prepared input pool.
    #[must_use]
    pub fn take_next(&mut self) -> ProjectionReplayBatch {
        self.batches.pop_front().unwrap_or_else(|| {
            panic!("prepared projection replay input exhausted after {PREPARED_PROJECTION_REPLAY_BATCH_COUNT} timed batches")
        })
    }
}

pub fn query_scaling_context(
    label: &str,
    dataset_rows: usize,
    aggregation_workers: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    let context = super::context::scaling_query_context_now(
        label,
        dataset_rows,
        aggregation_workers,
    )
    .and_then(|context| {
        super::join_context::prepare_scaling_join_collections(&context, dataset_rows)?;
        context.cassie.execute_sql(
            &context.session,
            "CREATE INDEX bench_documents_column_idx ON bench_documents USING column (title, body, status, score) WITH (segment_size = 256)",
            vec![],
        )?;
        Ok(context)
    });
    ready(context)
}

pub fn relational_query(ctx: &BenchContext) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let rows = query(ctx, RELATIONAL_SCALING_SQL, vec![Value::Int64(40)], 25).into_inner();
    let after = ctx.cassie.metrics();
    assert!(
        metric_delta(&before, &after, "read_paths", "range_scans")
            + metric_delta(&before, &after, "read_paths", "ordered_bounded_scans")
            > 0,
        "relational scaling query must use the scalar index"
    );
    ready(rows)
}

pub fn join_query(ctx: &BenchContext) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let rows = query(ctx, JOIN_SCALING_SQL, vec![], 50).into_inner();
    let after = ctx.cassie.metrics();
    assert!(
        metric_delta(&before, &after, "joins", "vectorized_joins") > 0,
        "join scaling query must use the vectorized join"
    );
    assert_eq!(
        metric_delta(&before, &after, "joins", "vectorized_fallbacks"),
        0,
        "join scaling query must not fall back"
    );
    ready(rows)
}

pub fn column_query(ctx: &BenchContext) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let rows = query(ctx, COLUMN_SCALING_SQL, vec![], 1).into_inner();
    let after = ctx.cassie.metrics();
    assert!(
        metric_delta(&before, &after, "aggregate_acceleration", "scans") > 0,
        "column scaling query must use aggregate acceleration"
    );
    ready(rows)
}

pub fn worker_query(ctx: &BenchContext, expected_workers: usize) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, WORKER_SCALING_SQL, vec![])
        .expect("worker saturation query");
    assert_eq!(result.rows.len(), 2, "worker query result cardinality");
    let after = ctx.cassie.metrics();
    if expected_workers > 1 {
        assert!(
            metric_delta(&before, &after, "parallel_aggregation", "aggregations") > 0,
            "worker scaling query must use parallel aggregation"
        );
        assert!(
            metric_delta(&before, &after, "parallel_aggregation", "workers")
                >= u64::try_from(expected_workers).expect("worker count should fit u64"),
            "worker scaling query must record the configured workers"
        );
        assert_eq!(
            metric_delta(
                &before,
                &after,
                "parallel_aggregation",
                "fallback_aggregations"
            ),
            0,
            "worker scaling query must not fall back"
        );
    }
    assert_scaling_resource_bounds(ctx);
    ready(std::hint::black_box(result.rows.len()))
}

fn metric_delta(
    before: &serde_json::Value,
    after: &serde_json::Value,
    family: &str,
    metric: &str,
) -> u64 {
    after[family][metric]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(before[family][metric].as_u64().unwrap_or_default())
}

pub fn full_text_query(ctx: &BenchContext) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let rows = query(ctx, FULLTEXT_SCALING_SQL, fulltext_scaling_params(), 20).into_inner();
    let after = ctx.cassie.metrics();
    assert!(
        metric_delta(&before, &after, "search", "posting_reads_total") > 0,
        "full-text scaling query must read persisted postings"
    );
    assert_eq!(
        metric_delta(&before, &after, "search", "row_scan_fallback_total"),
        0,
        "full-text scaling query must not fall back to a row scan"
    );
    ready(rows)
}

pub fn fulltext_scaling_params() -> Vec<Value> {
    vec![Value::String("alpha".to_string())]
}

pub fn vector_query(ctx: &BenchContext) -> Ready<usize> {
    vector_query_with_index(ctx, None)
}

pub fn vector_hnsw_query(ctx: &BenchContext) -> Ready<usize> {
    vector_query_with_index(ctx, Some("hnsw"))
}

pub fn vector_ivfflat_query(ctx: &BenchContext) -> Ready<usize> {
    vector_query_with_index(ctx, Some("ivfflat"))
}

fn vector_query_with_index(ctx: &BenchContext, index_kind: Option<&str>) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let rows = query(ctx, VECTOR_SCALING_SQL, vector_params(), 20).into_inner();
    let after = ctx.cassie.metrics();
    assert!(
        vector_execution_count_is_required(index_kind)
            && metric_delta(&before, &after, "vector", "count") > 0,
        "vector scaling query must record vector execution"
    );
    match index_kind {
        Some("hnsw") => {
            assert!(
                metric_delta(&before, &after, "vector", "hnsw_executions") > 0,
                "HNSW scaling query must use HNSW"
            );
            assert_eq!(
                metric_delta(&before, &after, "vector", "hnsw_fallbacks"),
                0,
                "HNSW scaling query must not fall back"
            );
        }
        Some("ivfflat") => {
            assert!(
                metric_delta(&before, &after, "vector", "ivfflat_executions") > 0,
                "IVFFlat scaling query must use IVFFlat"
            );
            assert_eq!(
                metric_delta(&before, &after, "vector", "ivfflat_fallbacks"),
                0,
                "IVFFlat scaling query must not fall back"
            );
        }
        None => {
            assert_eq!(
                metric_delta(&before, &after, "vector", "hnsw_executions")
                    + metric_delta(&before, &after, "vector", "ivfflat_executions"),
                0,
                "exact vector scaling query must not use ANN"
            );
            assert_eq!(
                metric_delta(&before, &after, "vector", "hnsw_fallbacks")
                    + metric_delta(&before, &after, "vector", "ivfflat_fallbacks"),
                0,
                "exact vector scaling query must not record ANN fallback contamination"
            );
        }
        Some(other) => panic!("unsupported vector scaling index kind '{other}'"),
    }
    ready(rows)
}

pub fn vector_execution_count_is_required(index_kind: Option<&str>) -> bool {
    matches!(index_kind, None | Some("hnsw" | "ivfflat"))
}

pub fn hybrid_query(ctx: &BenchContext) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let rows = query(
        ctx,
        HYBRID_SCALING_SQL,
        vec![
            Value::String("alpha".to_string()),
            Value::Vector(Vector::new(vec![1.0, 0.0, 0.0])),
        ],
        20,
    )
    .into_inner();
    let after = ctx.cassie.metrics();
    for metric in [
        "posting_reads_total",
        "ann_reads_total",
        "exact_reranks_total",
    ] {
        assert!(
            metric_delta(&before, &after, "hybrid", metric) > 0,
            "hybrid scaling query must record {metric}"
        );
    }
    assert_eq!(
        metric_delta(&before, &after, "hybrid", "prefilter_fallback_count_total"),
        0,
        "hybrid scaling query must not fall back"
    );
    ready(rows)
}

pub fn isolated_projection_replay_context(ctx: &BenchContext) -> BenchContext {
    ctx.cassie
        .execute_sql(
            &ctx.session,
            "CREATE TABLE bench_replay_source (title TEXT, score INT, status TEXT)",
            vec![],
        )
        .expect("create isolated replay source");
    ctx.cassie
        .execute_sql(
            &ctx.session,
            "CREATE MATERIALIZED PROJECTION bench_replay_projection AS SELECT title, score, status FROM bench_replay_source",
            vec![],
        )
        .expect("create isolated replay projection");
    let materialized = ctx
        .cassie
        .catalog
        .get_materialized_projection("bench_replay_projection")
        .expect("isolated replay projection metadata");
    let output_collection = materialized
        .active_output_collection()
        .or_else(|| {
            materialized
                .materialized
                .as_ref()
                .map(|metadata| metadata.output_collection.as_str())
        })
        .expect("isolated replay output collection")
        .to_string();
    let mut replay_metadata = ProjectionMeta::new(&output_collection, 1);
    replay_metadata.source_identity = Some("bench-replay-stream".to_string());
    ctx.cassie
        .midge
        .put_projection_metadata(&replay_metadata)
        .expect("persist isolated replay metadata");
    ctx.cassie
        .catalog
        .register_projection_metadata(replay_metadata);
    let mut replay_context = ctx.clone();
    replay_context.collection = output_collection;
    replay_context
}

/// Builds every replay payload before the timed fixed-duration closure starts.
#[must_use]
pub fn prepare_isolated_projection_replay_batches(
    ctx: &BenchContext,
) -> PreparedProjectionReplayBatches {
    let batches = (0..PREPARED_PROJECTION_REPLAY_BATCH_COUNT)
        .map(|nonce| prepare_isolated_projection_replay_batch(ctx, nonce))
        .collect();
    PreparedProjectionReplayBatches { batches }
}

fn prepare_isolated_projection_replay_batch(
    ctx: &BenchContext,
    nonce: usize,
) -> ProjectionReplayBatch {
    let events = (0..PROJECTION_REPLAY_EVENTS_PER_BATCH)
        .map(|index| ProjectionReplayEvent {
            event_id: format!("scale-replay-event-{nonce}-{index}"),
            checkpoint: format!("scale-replay-checkpoint-{nonce}-{index}"),
            position: None,
            document_id: format!("scale-replay-doc-{nonce}-{index}"),
            payload: Some(json!({
                "title": format!("scale-title-{nonce}-{index}"),
                "score": i64::try_from(index).expect("replay score should fit i64"),
                "status": "approved",
            })),
        })
        .collect::<Vec<_>>();
    ProjectionReplayBatch {
        projection: ctx.collection.clone(),
        source_identity: "bench-replay-stream".to_string(),
        batch_id: format!("scale-replay-batch-{nonce}"),
        lag: 0,
        events,
    }
}

pub fn isolated_projection_replay(
    ctx: &BenchContext,
    batch: ProjectionReplayBatch,
) -> Ready<usize> {
    let report = ctx
        .cassie
        .replay_projection_batch(batch)
        .expect("isolated scaling projection replay");
    assert_eq!(
        report.applied_event_count,
        u64::try_from(PROJECTION_REPLAY_EVENTS_PER_BATCH)
            .expect("projection replay event count should fit u64"),
        "isolated scaling projection replay cardinality"
    );
    ready(std::hint::black_box(PROJECTION_REPLAY_EVENTS_PER_BATCH))
}

pub fn drop_vector_index(ctx: &BenchContext) {
    ctx.cassie
        .execute_sql(
            &ctx.session,
            "DROP INDEX bench_documents_embedding_idx ON bench_documents",
            vec![],
        )
        .expect("drop benchmark vector index");
}

pub fn create_hnsw_index(ctx: &BenchContext) {
    ctx.cassie
        .execute_sql(
            &ctx.session,
            "CREATE INDEX bench_documents_embedding_idx ON bench_documents USING vector (embedding) WITH (source_field = body, metric = l2, index_type = hnsw, m = 32, ef_construction = 256, ef_search = 256)",
            vec![],
        )
        .expect("create benchmark HNSW index");
}

pub fn create_ivfflat_index(ctx: &BenchContext) {
    ctx.cassie
        .execute_sql(
            &ctx.session,
            "CREATE INDEX bench_documents_embedding_idx ON bench_documents USING vector (embedding) WITH (source_field = body, metric = l2, index_type = ivfflat, lists = 16, probes = 16, training_sample_size = 1024, training_seed = 42)",
            vec![],
        )
        .expect("create benchmark IVFFlat index");
}

pub fn bounded_mixed_operation(ctx: &BenchContext, nonce: usize) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let marker = format!("soak-marker-{nonce}");
    let id = ctx
        .cassie
        .ingest_document(
            &ctx.collection,
            json!({
                "title": marker,
                "body": "alpha beta gamma",
                "score": i64::try_from(nonce % 100).expect("score should fit i64"),
                "status": "approved",
                "embedding": [1.0, 0.0, 0.0],
            }),
        )
        .expect("bounded mixed ingest");
    let loaded = ctx
        .cassie
        .execute_sql(
            &ctx.session,
            "SELECT id FROM bench_documents WHERE title = $1 LIMIT 1",
            vec![Value::String(format!("soak-marker-{nonce}"))],
        )
        .expect("bounded mixed relational query");
    assert_eq!(loaded.rows.len(), 1, "mixed relational cardinality");
    let text_rows = full_text_query(ctx).into_inner();
    let vector_rows = vector_query(ctx).into_inner();
    let deleted = ctx
        .cassie
        .midge
        .delete_document(&ctx.collection, &id)
        .expect("bounded mixed cleanup");
    assert!(
        deleted,
        "bounded mixed cleanup must delete the ingested row"
    );

    let after = ctx.cassie.metrics();
    assert_eq!(
        after["execution_result_cache"]["hits"], before["execution_result_cache"]["hits"],
        "mixed soak must not use execution-result caching"
    );
    assert_scaling_resource_bounds(ctx);
    ready(std::hint::black_box(1 + text_rows + vector_rows))
}

fn query(ctx: &BenchContext, sql: &str, params: Vec<Value>, expected_rows: usize) -> Ready<usize> {
    let before = ctx.cassie.metrics();
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, params)
        .expect("benchmark query");
    assert_eq!(
        result.rows.len(),
        expected_rows,
        "benchmark query result cardinality"
    );
    let after = ctx.cassie.metrics();
    assert_eq!(
        after["execution_result_cache"]["hits"], before["execution_result_cache"]["hits"],
        "benchmark query used the execution-result cache"
    );
    assert_scaling_resource_bounds(ctx);
    ready(std::hint::black_box(result.rows.len()))
}

pub fn assert_scaling_resource_bounds(ctx: &BenchContext) {
    assert_scaling_cassie_resource_bounds(&ctx.cassie);
}

pub fn assert_scaling_cassie_resource_bounds(cassie: &Cassie) {
    let metrics = cassie.metrics();
    assert_eq!(
        metrics["execution_result_cache"]["hits"].as_u64(),
        Some(0),
        "scaling benchmark must not use execution-result caching"
    );
    assert_eq!(
        metrics["execution_result_cache"]["entries"].as_u64(),
        Some(0),
        "scaling benchmark execution-result cache must stay empty"
    );
    assert!(
        metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .unwrap_or_default()
            <= 64 * 1024 * 1024,
        "scaling benchmark exceeded the benchmark query-memory bound"
    );
    assert_eq!(
        metrics["runtime"]["running_queries"].as_u64(),
        Some(0),
        "scaling benchmark leaked an active query"
    );
    assert_eq!(
        metrics["runtime"]["active_operator_workers"].as_u64(),
        Some(0),
        "scaling benchmark leaked operator-worker permits"
    );
    assert_eq!(
        metrics["query"]["errors_total"].as_u64(),
        Some(0),
        "scaling benchmark recorded a query error"
    );
    assert_eq!(
        metrics["runtime"]["query_admission_rejections"].as_u64(),
        Some(0),
        "scaling benchmark exceeded its query-admission bound"
    );
}

pub async fn wait_for_pgwire_session_cleanup(ctx: &BenchContext) {
    for _ in 0..200 {
        if ctx.cassie.metrics()["pgwire"]["active_sessions"].as_u64() == Some(0) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        ctx.cassie.metrics()["pgwire"]["active_sessions"].as_u64(),
        Some(0),
        "pgwire benchmark sessions must close before the cleanup deadline"
    );
}

fn vector_params() -> Vec<Value> {
    vec![Value::Vector(Vector::new(vec![1.0, 0.0, 0.0]))]
}
