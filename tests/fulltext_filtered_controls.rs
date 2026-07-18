use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use serde_json::json;
use uuid::Uuid;

#[path = "support/pgwire.rs"]
mod wire;

const COLLECTION: &str = "fulltext_filtered_controls";
const QUERY: &str = "SELECT id, body, search_score(body, 'alpha') AS score \
    FROM fulltext_filtered_controls \
    WHERE search(body, 'alpha') AND body <> 'never' LIMIT 8";
const FIXTURE_ROWS: usize = 64;
const LOW_MEMORY_BYTES: usize = 1_024;
const NORMAL_MEMORY_BYTES: usize = 4 * 1024 * 1024;

struct Fixture {
    cassie: Cassie,
    path: String,
}

impl Fixture {
    fn new(memory_budget: usize) -> Self {
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
        let path = std::env::temp_dir()
            .join(format!(
                "cassie-fulltext-filtered-controls-{}",
                Uuid::new_v4()
            ))
            .to_string_lossy()
            .into_owned();
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = memory_budget;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {COLLECTION} (body TEXT, category TEXT)"),
                vec![],
            )
            .expect("create filtered fulltext table");
        let rows = (0..FIXTURE_ROWS)
            .map(|index| {
                (
                    Some(format!("row-{index:04}")),
                    json!({
                        "body": format!(
                            "alpha controlled filtered fulltext row {index:04} {}",
                            "bounded-payload-".repeat(12)
                        ),
                        "category": "included",
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(COLLECTION, rows)
            .expect("seed filtered fulltext rows");
        Self { cassie, path }
    }

    fn cleanup(self) {
        drop(self.cassie);
        let _ = std::fs::remove_dir_all(self.path);
    }
}

fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
    metrics[family][name].as_u64().unwrap_or_default()
}

fn assert_failed_metrics_unchanged(before: &serde_json::Value, after: &serde_json::Value) {
    for name in [
        "count",
        "candidate_count_total",
        "result_count_total",
        "retrieval_stage_queries_total",
        "posting_reads_total",
        "candidate_row_fetches_total",
        "row_scan_fallback_total",
    ] {
        assert_eq!(
            metric(after, "search", name),
            metric(before, "search", name),
            "filtered fulltext published failed-path metric search.{name}"
        );
    }
    assert_eq!(
        metric(after, "query", "rows_returned_total"),
        metric(before, "query", "rows_returned_total"),
        "filtered fulltext published partial rows"
    );
}

fn assert_cleanup(cassie: &Cassie) {
    let metrics = cassie.metrics();
    assert_eq!(metric(&metrics, "runtime", "running_queries"), 0);
    assert_eq!(
        metric(&metrics, "query", "current_accounted_memory_bytes"),
        0
    );
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field, _)| *field == tag)
        .map(|(_, value)| value.as_str())
}

fn wire_sqlstate(memory_budget: usize, cancel_after_reads: Option<usize>) -> String {
    let fixture = Fixture::new(memory_budget);
    let Fixture { cassie, path } = fixture;
    let runtime = current_thread_runtime();
    let sqlstate = runtime.block_on(async {
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        set_query_scan_cancellation_after_entries(cancel_after_reads);
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(QUERY)]).await;
        let frames = wire::read_frames_until_ready(&mut reader).await;
        set_query_scan_cancellation_after_entries(None);
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("pgwire filtered fulltext error");
        let fields = wire::parse_error_fields(&error.1);
        let sqlstate = error_field(&fields, 'C')
            .expect("SQLSTATE error field")
            .to_string();
        server.stop().await;
        sqlstate
    });
    let _ = std::fs::remove_dir_all(path);
    sqlstate
}

#[test]
fn should_reject_filtered_fulltext_before_retaining_the_exact_source() {
    let _guard = query_scan_control_test_guard();

    // Arrange
    let fixture = Fixture::new(LOW_MEMORY_BYTES);
    let session = fixture.cassie.create_session("reader", None);
    let before = fixture.cassie.metrics();
    let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let error = fixture
        .cassie
        .execute_sql(&session, QUERY, vec![])
        .expect_err("low-budget filtered fulltext query should be atomic");
    let after = fixture.cassie.metrics();
    let reads = fixture
        .cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before_reads);

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    assert!(
        reads <= 2,
        "low-memory filtered fulltext read bound: {reads}"
    );
    assert_failed_metrics_unchanged(&before, &after);
    assert_cleanup(&fixture.cassie);
    fixture.cleanup();
    assert_eq!(wire_sqlstate(LOW_MEMORY_BYTES, None), "54000");
}

#[test]
fn should_cancel_filtered_fulltext_after_three_exact_source_reads() {
    let _guard = query_scan_control_test_guard();

    // Arrange
    let fixture = Fixture::new(NORMAL_MEMORY_BYTES);
    let session = fixture.cassie.create_session("reader", None);
    let before = fixture.cassie.metrics();
    let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(3));

    // Act
    let error = fixture
        .cassie
        .execute_sql(&session, QUERY, vec![])
        .expect_err("filtered fulltext exact source should cancel deterministically");
    set_query_scan_cancellation_after_entries(None);
    let after = fixture.cassie.metrics();
    let reads = fixture
        .cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before_reads);

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(reads, 3);
    assert_failed_metrics_unchanged(&before, &after);
    assert_cleanup(&fixture.cassie);
    fixture.cleanup();
    assert_eq!(wire_sqlstate(NORMAL_MEMORY_BYTES, Some(3)), "57014");
}
