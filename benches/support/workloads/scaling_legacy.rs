use std::future::{ready, Ready};
use std::path::PathBuf;

use cassie::app::CassieError;
use cassie::types::Value;

use super::context::{
    reopen_scaling_query_context_now, scaling_query_disk_context_now, BenchContext,
};
use super::scaling::assert_scaling_resource_bounds;

pub const SIMPLE_SCALING_SQL: &str = "SELECT id, title FROM bench_documents WHERE id = $1";
pub const MIXED_DIRECTION_SCALING_SQL: &str =
    "SELECT id FROM bench_documents ORDER BY score DESC, id ASC LIMIT 50";
pub const EXPRESSION_INDEX_SCALING_SQL: &str =
    "SELECT id FROM bench_documents WHERE lower(title) = $1 LIMIT 50";
pub const EXPRESSION_INDEX_RANGE_SCALING_SQL: &str =
    "SELECT id FROM bench_documents WHERE lower(title) >= $1 AND lower(title) < $2 LIMIT 50";
pub const EXPRESSION_INDEX_ORDER_SCALING_SQL: &str =
    "SELECT id FROM bench_documents ORDER BY lower(title) ASC LIMIT 50";
pub const WINDOW_FRAME_SCALING_SQL: &str = "SELECT status, score, first_value(score) OVER (PARTITION BY status ORDER BY score ROWS BETWEEN 3 PRECEDING AND 3 FOLLOWING) AS first_score, last_value(score) OVER (PARTITION BY status ORDER BY score ROWS BETWEEN 3 PRECEDING AND 3 FOLLOWING) AS last_score FROM bench_documents ORDER BY status, score, id";
pub const LEFT_JOIN_SCALING_SQL: &str = "SELECT bench_join_users.name, bench_join_orders.total FROM bench_join_users LEFT JOIN bench_join_orders ON bench_join_users.user_key = bench_join_orders.order_user_key LIMIT 50";
pub const INNER_JOIN_2_SCALING_SQL: &str = "SELECT bench_join_users.name, bench_join_orders.total FROM bench_join_users JOIN bench_join_orders ON bench_join_users.user_key = bench_join_orders.order_user_key LIMIT 2";
pub const INNER_JOIN_50_SCALING_SQL: &str = "SELECT bench_join_users.name, bench_join_orders.total FROM bench_join_users JOIN bench_join_orders ON bench_join_users.user_key = bench_join_orders.order_user_key LIMIT 50";
pub const SPARSE_JOIN_SCALING_SQL: &str = "SELECT bench_sparse_users.name, bench_sparse_orders.total FROM bench_sparse_users JOIN bench_sparse_orders ON bench_sparse_users.user_key = bench_sparse_orders.order_user_key LIMIT 50";
pub const DENSE_JOIN_SCALING_SQL: &str = "SELECT bench_dense_users.name, bench_dense_orders.total FROM bench_dense_users JOIN bench_dense_orders ON bench_dense_users.user_key = bench_dense_orders.order_user_key LIMIT 2";
pub const LATE_MATCH_JOIN_SCALING_SQL: &str = "SELECT bench_late_users.name, bench_late_orders.total FROM bench_late_users JOIN bench_late_orders ON bench_late_users.user_key = bench_late_orders.order_user_key LIMIT 50";
pub const FANOUT_JOIN_SCALING_SQL: &str = "SELECT bench_fanout_users.name, bench_fanout_orders.total FROM bench_fanout_users JOIN bench_fanout_orders ON bench_fanout_users.user_key = bench_fanout_orders.order_user_key LIMIT 500";

pub fn query_scaling_disk_context(
    label: &str,
    dataset_rows: usize,
    aggregation_workers: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    let context = scaling_query_disk_context_now(label, dataset_rows, aggregation_workers)
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

pub struct QueryScalingFixture {
    data_dir: PathBuf,
    expected_rows: usize,
    cleaned: bool,
}

impl QueryScalingFixture {
    #[must_use]
    pub fn close(context: BenchContext, expected_rows: usize) -> Self {
        assert_fixture_boundaries(&context, expected_rows);
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        Self {
            data_dir,
            expected_rows,
            cleaned: false,
        }
    }

    /// Reopens the one persisted fixture with a different worker profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the persisted database cannot be reopened.
    pub fn reopen(&self, workers: usize) -> Result<BenchContext, CassieError> {
        let context = reopen_scaling_query_context_now(
            self.data_dir.clone(),
            self.expected_rows,
            workers,
            super::context::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
            1_024,
        )?;
        assert_fixture_boundaries(&context, self.expected_rows);
        Ok(context)
    }

    /// Reopens the same storage with the dense-stream selection profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the persisted database cannot be reopened.
    pub fn reopen_dense_stream(&self) -> Result<BenchContext, CassieError> {
        let context = reopen_scaling_query_context_now(
            self.data_dir.clone(),
            self.expected_rows,
            1,
            4 * 1_024,
            8,
        )?;
        assert_fixture_boundaries(&context, self.expected_rows);
        Ok(context)
    }

    /// Removes the persisted benchmark fixture.
    ///
    /// # Errors
    ///
    /// Returns an error when the fixture cannot be removed.
    pub fn cleanup(mut self) -> Result<(), CassieError> {
        self.remove_data_dir()
            .map_err(|error| CassieError::Execution(error.to_string()))
    }

    fn remove_data_dir(&mut self) -> std::io::Result<()> {
        if self.data_dir.is_dir() {
            std::fs::remove_dir_all(&self.data_dir)?;
        } else if self.data_dir.exists() {
            std::fs::remove_file(&self.data_dir)?;
        }
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for QueryScalingFixture {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.remove_data_dir();
        }
    }
}

pub fn execute_legacy_query(
    context: &BenchContext,
    sql: &str,
    params: Vec<Value>,
    expected_rows: usize,
) -> usize {
    let result = context
        .cassie
        .execute_sql(&context.session, sql, params)
        .expect("execute legacy scaling query");
    assert_eq!(
        result.rows.len(),
        expected_rows,
        "legacy scaling query cardinality"
    );
    assert_scaling_resource_bounds(context);
    std::hint::black_box(result.rows.len())
}

pub fn prepare_recursive_cte_scaling(context: &BenchContext) {
    if context.cassie.catalog.exists("recursive_cte_fanout") {
        return;
    }
    context
        .cassie
        .execute_sql(
            &context.session,
            "CREATE TABLE recursive_cte_fanout (n INT)",
            vec![],
        )
        .expect("create recursive CTE scaling table");
    for _ in 0..10 {
        context
            .cassie
            .execute_sql(
                &context.session,
                "INSERT INTO recursive_cte_fanout (n) VALUES ($1)",
                vec![Value::Int64(1)],
            )
            .expect("seed recursive CTE scaling table");
    }
}

pub fn execute_legacy_join_query(
    context: &BenchContext,
    workload: &str,
    sql: &str,
    expected_rows: usize,
) -> usize {
    let before = context.cassie.metrics();
    let rows = execute_legacy_query(context, sql, vec![], expected_rows);
    let after = context.cassie.metrics();
    assert!(
        metric_delta(&before, &after, "joins", "vectorized_joins") > 0,
        "legacy join scaling query must use the vectorized join"
    );
    assert_eq!(
        metric_delta(&before, &after, "joins", "vectorized_fallbacks"),
        0,
        "legacy join scaling query must not fall back"
    );
    assert_legacy_join_variant(&before, &after, workload);
    rows
}

fn assert_legacy_join_variant(
    before: &serde_json::Value,
    after: &serde_json::Value,
    workload: &str,
) {
    let build_rows = metric_delta(before, after, "joins", "vectorized_build_rows_total");
    let probe_rows = metric_delta(before, after, "joins", "vectorized_probe_rows_total");
    let index_seeks = metric_delta(before, after, "read_paths", "index_seek_scans");
    let last_collection = after["read_paths"]["last_collection_scan_collection"]
        .as_str()
        .unwrap_or_default();
    let last_index_collection = after["read_paths"]["last_index_scan_collection"]
        .as_str()
        .unwrap_or_default();
    let selection_reason = after["joins"]["last_bounded_side_selection_reason"]
        .as_str()
        .unwrap_or_default();
    match workload {
        "vectorized_left_join_limited" => {
            assert!(
                build_rows > 0 && probe_rows > 0,
                "bounded left join must build and probe"
            );
            assert_eq!(index_seeks, 0, "bounded left join must scan its sources");
        }
        "vectorized_streaming_inner_join" => {
            assert!(
                build_rows <= 50,
                "streaming join must build the sparse source"
            );
            assert!(
                last_collection.ends_with("bench_sparse_users"),
                "streaming join must scan the large left source: {last_collection}"
            );
        }
        "vectorized_dense_streaming_inner_join" => {
            assert_eq!(
                selection_reason, "dense_stream_preemptive_temp_budget",
                "dense join must select the 4 KiB dense-stream path"
            );
        }
        "vectorized_indexed_inner_join" => {
            assert!(
                index_seeks > 0,
                "indexed-left join must seek the left index"
            );
            assert!(
                last_index_collection.ends_with("bench_join_users"),
                "indexed-left join selected the wrong index source: {last_index_collection}"
            );
        }
        "vectorized_right_indexed_inner_join" => {
            assert!(
                index_seeks > 0,
                "indexed-right join must seek the right index"
            );
            assert!(
                last_index_collection.ends_with("bench_join_orders"),
                "indexed-right join selected the wrong index source: {last_index_collection}"
            );
        }
        "vectorized_late_match_inner_join" => {
            assert_eq!(
                selection_reason, "left_build_bounded_row_count_probe",
                "late-match join must use the bounded row-count probe"
            );
            assert!(
                build_rows <= 50,
                "late-match join must build the bounded left side"
            );
        }
        "vectorized_fanout_inner_join" => {
            assert!(
                build_rows <= 100_000 / 3,
                "fanout join must build the smaller left side"
            );
            assert!(
                last_collection.ends_with("bench_fanout_orders"),
                "fanout join must stream the right source: {last_collection}"
            );
        }
        other => panic!("unsupported legacy join scaling workload '{other}'"),
    }
}

pub fn prepare_fulltext_warm_state(context: &BenchContext) {
    let rows = execute_legacy_query(
        context,
        super::scaling::FULLTEXT_SCALING_SQL,
        super::scaling::fulltext_scaling_params(),
        20,
    );
    assert_eq!(rows, 20, "full-text warm-state cardinality");
}

pub fn prepare_projection_lifecycle(context: &BenchContext) {
    context
        .cassie
        .execute_sql(
            &context.session,
            "CREATE MATERIALIZED PROJECTION IF NOT EXISTS bench_projection AS SELECT title, score, status FROM bench_documents",
            vec![],
        )
        .expect("prepare scaling projection");
    let rows = projection_refresh_existing(context).into_inner();
    assert!(rows > 0, "projection setup refresh must complete");
}

pub fn prepare_time_series_lifecycle_context(
    context: &BenchContext,
    dataset_rows: usize,
) -> BenchContext {
    const COLLECTION: &str = "bench_time_series_events";
    if !context.cassie.catalog.exists(COLLECTION) {
        context
            .cassie
            .execute_sql(
                &context.session,
                "CREATE TABLE bench_time_series_events (tenant TEXT, event_at TIMESTAMP, amount INT, status TEXT)",
                vec![],
            )
            .expect("create scaling time-series collection");
        for statement in [
            "CREATE INDEX bench_time_series_time_idx ON bench_time_series_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
            "CREATE ROLLUP bench_time_series_hourly ON bench_time_series_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
            "CREATE RETENTION POLICY bench_time_series_retention ON bench_time_series_events USING event_at RETAIN FOR '2 days'",
        ] {
            context
                .cassie
                .execute_sql(&context.session, statement, vec![])
                .expect("prepare scaling time-series metadata");
        }
        let tenants = ["tenant-a", "tenant-b", "tenant-c", "tenant-d"];
        let documents = (0..dataset_rows)
            .map(|index| {
                let day = 9 + ((index / 24) % 7);
                let hour = index % 24;
                (
                    Some(format!("ts-doc-{index}")),
                    serde_json::json!({
                        "tenant": tenants[index % tenants.len()],
                        "event_at": format!("2026-01-{day:02}T{hour:02}:00:00Z"),
                        "amount": i64::try_from(index % 100).expect("amount should fit i64"),
                        "status": if index % 2 == 0 { "open" } else { "closed" },
                    }),
                )
            })
            .collect::<Vec<_>>();
        context
            .cassie
            .midge
            .put_fresh_time_series_documents(COLLECTION, documents)
            .expect("seed scaling time-series collection");
        context
            .cassie
            .midge
            .put_documents(
                COLLECTION,
                vec![(
                    Some("ts-retention-expired-sentinel".to_string()),
                    serde_json::json!({
                        "tenant": "tenant-retention",
                        "event_at": "2026-01-01T00:00:00Z",
                        "amount": 1,
                        "status": "expired-sentinel",
                    }),
                )],
            )
            .expect("seed expired retention sentinel");
    }
    let mut time_series = context.clone();
    time_series.collection = COLLECTION.to_string();
    time_series
}

pub fn projection_refresh_existing(context: &BenchContext) -> Ready<usize> {
    let before = context.cassie.metrics();
    let result = context
        .cassie
        .execute_sql(
            &context.session,
            "REFRESH MATERIALIZED PROJECTION bench_projection",
            vec![],
        )
        .expect("refresh existing scaling projection");
    let after = context.cassie.metrics();
    assert!(
        metric_delta(&before, &after, "projections", "materialized_refreshes") > 0,
        "scaling projection refresh must record a refresh"
    );
    assert_scaling_resource_bounds(context);
    assert!(
        !result.command.is_empty(),
        "projection refresh command report"
    );
    ready(std::hint::black_box(usize::from(
        !result.command.is_empty(),
    )))
}

pub fn projection_verify_existing(context: &BenchContext) -> Ready<usize> {
    let before = context.cassie.metrics();
    let result = context
        .cassie
        .execute_sql(
            &context.session,
            "VERIFY PROJECTION bench_projection MODE full",
            vec![],
        )
        .expect("verify existing scaling projection");
    let after = context.cassie.metrics();
    assert!(
        metric_delta(&before, &after, "projections", "integrity_verifications") > 0,
        "scaling projection verification must record a verification"
    );
    assert_scaling_resource_bounds(context);
    assert_eq!(result.rows.len(), 1, "projection verification report");
    ready(std::hint::black_box(result.rows.len()))
}

fn assert_fixture_boundaries(context: &BenchContext, expected_rows: usize) {
    assert!(expected_rows > 0, "scaling fixture must not be empty");
    for id in ["doc-0".to_string(), format!("doc-{}", expected_rows - 1)] {
        assert!(
            context
                .cassie
                .midge
                .get_document("bench_documents", &id)
                .expect("read scaling fixture boundary")
                .is_some(),
            "scaling fixture must contain boundary document '{id}'"
        );
    }
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
