use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

#[path = "support/sql.rs"]
mod support;

const COLLECTION: &str = "controlled_column_metadata";
const METADATA_REJECTION_BUDGET: usize = 1_024;

struct Fixture {
    cassie: Cassie,
    path: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn fixture() -> Fixture {
    support::with_fallback();
    let path = support::data_dir("column-metadata-controls");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = METADATA_REJECTION_BUDGET;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("controlled cassie");
    cassie.startup().expect("startup controlled cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {COLLECTION} (score INT, label TEXT)"),
            vec![],
        )
        .expect("create controlled table");
    let rows = (0..64)
        .map(|index| {
            (
                Some(format!("row-{index:04}")),
                serde_json::json!({
                    "score": index,
                    "label": format!("label-{index:04}-{}", "x".repeat(512)),
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(COLLECTION, rows)
        .expect("seed controlled rows");
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE INDEX controlled_column_metadata_idx ON {COLLECTION} USING column \
                 (score, label) WITH (segment_size = 1)"
            ),
            vec![],
        )
        .expect("create metadata-heavy column index");
    Fixture { cassie, path }
}

fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
    metrics[family][name].as_u64().unwrap_or_default()
}

fn execute_with_first_segment_cancellation(
    fixture: &Fixture,
    sql: &str,
) -> (Result<cassie::executor::QueryResult, CassieError>, u64) {
    let session = fixture.cassie.create_session("reader", None);
    let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
    cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(1));
    let result = fixture.cassie.execute_sql(&session, sql, vec![]);
    cassie::midge::adapter::set_query_scan_cancellation_after_entries(None);
    let reads = fixture
        .cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before_reads);
    (result, reads)
}

fn assert_resource_rejection_before_segment_reads(
    fixture: &Fixture,
    result: Result<cassie::executor::QueryResult, CassieError>,
    reads: u64,
    before: &serde_json::Value,
    metric_family: &str,
) {
    let error = result.expect_err("metadata-heavy query should reject before segment reads");
    assert!(matches!(error, CassieError::ResourceLimit(_)), "{error:?}");
    assert_eq!(reads, 0, "segment scan started before metadata reservation");
    let after = fixture.cassie.metrics();
    assert_eq!(
        metric(&after, metric_family, "scans"),
        metric(before, metric_family, "scans"),
        "failed metadata path published success"
    );
    assert_eq!(metric(&after, "runtime", "running_queries"), 0);
    assert_eq!(metric(&after, "query", "current_accounted_memory_bytes"), 0);
}

#[test]
fn should_reserve_projection_metadata_before_loading_column_segments() {
    let _guard = cassie::midge::adapter::query_scan_control_test_guard();
    // Arrange
    let fixture = fixture();
    let before = fixture.cassie.metrics();

    // Act
    let (result, reads) = execute_with_first_segment_cancellation(
        &fixture,
        &format!("SELECT id, score, label FROM {COLLECTION} WHERE score >= 0 LIMIT 5"),
    );

    // Assert
    assert_resource_rejection_before_segment_reads(
        &fixture,
        result,
        reads,
        &before,
        "column_batches",
    );
}

#[test]
fn should_reserve_aggregate_metadata_before_validating_column_segments() {
    let _guard = cassie::midge::adapter::query_scan_control_test_guard();
    // Arrange
    let fixture = fixture();
    let before = fixture.cassie.metrics();

    // Act
    let (result, reads) = execute_with_first_segment_cancellation(
        &fixture,
        &format!("SELECT COUNT(*), SUM(score), AVG(score) FROM {COLLECTION}"),
    );

    // Assert
    assert_resource_rejection_before_segment_reads(
        &fixture,
        result,
        reads,
        &before,
        "aggregate_acceleration",
    );
}
