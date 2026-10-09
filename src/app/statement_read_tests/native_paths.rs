//! Captured staged membership survives live rollback on native read paths.
use super::{execute, CaptureHook, Cassie, CassieSession, Value};
use crate::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict native fixture cleanup");
    }
}
struct Fixture {
    cassie: Arc<Cassie>,
    _directory: Directory,
}
impl Fixture {
    fn new(setup: &[&str]) -> Self {
        let directory = Directory(
            std::env::temp_dir().join(format!("cassie-native-view-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&directory.0).expect("create fixture directory");
        let mut config = CassieRuntimeConfig::default();
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        if setup.iter().any(|sql| sql.contains("USING vector")) {
            config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
                model: "deterministic-test".to_string(),
                dimensions: 2,
            });
        }
        let cassie =
            Arc::new(Cassie::new_with_data_dir_and_config(&directory.0, config).expect("Cassie"));
        cassie.startup().expect("startup");
        let session = cassie.create_session("setup", None);
        execute(
            &cassie,
            &session,
            "CREATE TABLE native_rows (id INT PRIMARY KEY, n INT, body TEXT, embedding VECTOR(2))",
        );
        insert(&cassie, &session, 1, 10);
        for sql in setup {
            execute(&cassie, &session, sql);
        }
        Self {
            cassie,
            _directory: directory,
        }
    }
}
fn insert(cassie: &Cassie, session: &CassieSession, id: i64, n: i64) {
    cassie
        .execute_sql(
            session,
            "INSERT INTO native_rows (id, n, body, embedding) VALUES ($1, $2, $3, $4)",
            vec![
                Value::Int64(id),
                Value::Int64(n),
                Value::String("alpha beta".to_string()),
                Value::Vector(crate::types::Vector::new(vec![1.0, 0.0])),
            ],
        )
        .expect("insert native fixture row");
}
struct Observation {
    original: Vec<Vec<Value>>,
    staged: Vec<Vec<Value>>,
    captured: Vec<Vec<Value>>,
    fresh: Vec<Vec<Value>>,
    rolled_back: bool,
}
fn observe(setup: &[&str], sql: &'static str) -> Observation {
    let fixture = Fixture::new(setup);
    let reader = fixture.cassie.create_session("reader", None);
    let original = execute(&fixture.cassie, &reader, sql);
    execute(&fixture.cassie, &reader, "BEGIN");
    insert(&fixture.cassie, &reader, 2, 20);
    let staged = execute(&fixture.cassie, &reader, sql);
    let cassie = Arc::clone(&fixture.cassie);
    let same_session = reader.clone();
    let rolled_back = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&rolled_back);
    let hook = CaptureHook::install(move || {
        execute(&cassie, &same_session, "ROLLBACK");
        observed.store(true, Ordering::SeqCst);
    });
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    drop(fixture);
    Observation {
        original,
        staged,
        captured,
        fresh,
        rolled_back: rolled_back.load(Ordering::SeqCst),
    }
}
fn assert_observation(result: &Observation) {
    assert!(result.rolled_back, "live rollback completed after capture");
    assert_ne!(
        result.staged, result.original,
        "the staged read oracle differs"
    );
    assert_eq!(
        result.fresh, result.original,
        "the subsequent statement observes rollback"
    );
    assert_eq!(
        result.captured, result.staged,
        "read selection retains captured staged membership"
    );
}

#[test]
fn should_keep_captured_overlay_for_persisted_fulltext_top_k() {
    // Arrange
    let setup = ["CREATE INDEX native_text ON native_rows USING fulltext (body)"];
    let sql = "SELECT _id, search_score(body, 'alpha') AS score FROM native_rows WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 10";
    // Act
    let result = observe(&setup, sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_keep_captured_overlay_for_persisted_fulltext_filter() {
    // Arrange
    let setup = ["CREATE INDEX native_text ON native_rows USING fulltext (body)"];
    let sql = "SELECT n, search_score(body, 'alpha') AS score FROM native_rows WHERE search(body, 'alpha') LIMIT 10";
    // Act
    let result = observe(&setup, sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_keep_captured_overlay_for_scalar_index_reads() {
    // Arrange
    let setup = ["CREATE INDEX native_n ON native_rows USING btree (n)"];
    let sql = "SELECT n FROM native_rows WHERE n >= 0";
    // Act
    let result = observe(&setup, sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_keep_captured_overlay_for_ordered_row_id_pages() {
    // Arrange
    let sql = "SELECT n FROM native_rows ORDER BY _id LIMIT 10";
    // Act
    let result = observe(&[], sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_keep_captured_overlay_for_column_summary_aggregation() {
    // Arrange
    let setup =
        ["CREATE INDEX native_column ON native_rows USING column (n) WITH (segment_size = 2)"];
    let sql = "SELECT SUM(n) AS total FROM native_rows";
    // Act
    let result = observe(&setup, sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_keep_captured_overlay_for_vector_retrieval() {
    // Arrange
    let setup = ["CREATE INDEX native_vector ON native_rows USING vector (embedding) WITH (source_field = body, index_type = hnsw, metric = l2)"];
    let sql = "SELECT _id, vector_distance(embedding, '[1,0]') AS distance FROM native_rows ORDER BY distance ASC LIMIT 10";
    // Act
    let result = observe(&setup, sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_keep_captured_overlay_for_hybrid_retrieval() {
    // Arrange
    let setup = ["CREATE INDEX native_text ON native_rows USING fulltext (body)", "CREATE INDEX native_vector ON native_rows USING vector (embedding) WITH (source_field = body, index_type = hnsw, metric = cosine)"];
    let sql = "SELECT _id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0]')) AS score FROM native_rows ORDER BY score DESC LIMIT 10";
    // Act
    let result = observe(&setup, sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_keep_captured_overlay_for_analytical_projection_reads() {
    // Arrange
    let setup = ["CREATE MATERIALIZED PROJECTION native_projection WITH (analytical = true) AS SELECT id, n FROM native_rows"];
    let sql = "SELECT id, n FROM native_rows WHERE n >= 0";
    // Act
    let result = observe(&setup, sql);
    // Assert
    assert_observation(&result);
}
#[test]
fn should_merge_captured_overlay_in_projected_batched_scan_helpers() {
    // Arrange
    let fixture = Fixture::new(&[]);
    let reader = fixture.cassie.create_session("reader", None);
    let collection = format!("{}.public.native_rows", fixture.cassie.default_database);
    execute(&fixture.cassie, &reader, "BEGIN");
    insert(&fixture.cassie, &reader, 2, 20);
    let controls = fixture
        .cassie
        .runtime
        .query_controls(std::time::Instant::now());
    let owner = reader
        .capture_statement_read(
            &fixture.cassie.midge,
            &fixture.cassie.default_database,
            &controls,
        )
        .expect("capture helper view");
    let read_scope = crate::midge::adapter::StatementReadScope::enter(Some(&owner));
    let overlay_scope = crate::app::SessionReadScope::enter(owner.overlay());
    let read = || {
        fixture
            .cassie
            .scan_projected_documents_batched_for_session_with_filter_and_timings(
                Some(&reader),
                &collection,
                128,
                &["n".to_string()],
                None,
                None,
            )
            .expect("projected scan")
            .0
            .into_iter()
            .flatten()
            .map(|document| document.payload["n"].as_i64().expect("n"))
            .collect::<Vec<_>>()
    };
    // Act
    execute(&fixture.cassie, &reader, "ROLLBACK");
    let mut captured = read();
    drop(overlay_scope);
    drop(read_scope);
    drop(owner);
    let fresh = read();
    let released = controls.current_query_memory_bytes();
    drop(fixture);
    captured.sort_unstable();
    // Assert
    assert_eq!(fresh, vec![10]);
    assert_eq!(released, 0);
    assert_eq!(captured, vec![10, 20]);
}

#[test]
fn should_keep_captured_overlay_for_time_series_ranges() {
    // Arrange
    let fixture = Fixture::new(&[]);
    let reader = fixture.cassie.create_session("time-reader", None);
    execute(
        &fixture.cassie,
        &reader,
        "CREATE TABLE native_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
    );
    execute(
        &fixture.cassie,
        &reader,
        "INSERT INTO native_events VALUES ('acme', '2026-01-01T00:00:00Z', 10)",
    );
    execute(&fixture.cassie, &reader, "CREATE INDEX native_events_time ON native_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)");
    let sql = "SELECT amount FROM native_events WHERE tenant = 'acme' AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-01T01:00:00Z' ORDER BY amount";
    let original = execute(&fixture.cassie, &reader, sql);
    execute(&fixture.cassie, &reader, "BEGIN");
    execute(
        &fixture.cassie,
        &reader,
        "INSERT INTO native_events VALUES ('acme', '2026-01-01T00:01:00Z', 20)",
    );
    let staged = execute(&fixture.cassie, &reader, sql);
    let cassie = Arc::clone(&fixture.cassie);
    let session = reader.clone();
    let rolled_back = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&rolled_back);
    let hook = CaptureHook::install(move || {
        execute(&cassie, &session, "ROLLBACK");
        observed.store(true, Ordering::SeqCst);
    });
    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    let metrics = fixture.cassie.metrics();
    // Assert
    assert!(rolled_back.load(Ordering::SeqCst));
    assert_eq!(original, vec![vec![Value::Int64(10)]]);
    assert_eq!(staged, vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]);
    assert_eq!(captured, staged);
    assert_eq!(fresh, original);
    assert!(
        metrics["time_series"]["bucket_native_hits"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

fn assert_commit_artifact_view(setup: &[&str], sql: &'static str) {
    assert_artifact_view_after_mutation(setup, sql, |cassie, writer| insert(cassie, writer, 2, 20));
}

fn assert_artifact_view_after_mutation(
    setup: &[&str],
    sql: &'static str,
    mutation: impl FnOnce(&Cassie, &CassieSession) + 'static,
) {
    let fixture = Fixture::new(setup);
    let reader = fixture.cassie.create_session("artifact-reader", None);
    let writer = fixture.cassie.create_session("artifact-writer", None);
    let original = execute(&fixture.cassie, &reader, sql);
    let cassie = Arc::clone(&fixture.cassie);
    let warm = Arc::new(std::sync::Mutex::new(None));
    let published = Arc::clone(&warm);
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    let hook = CaptureHook::install(move || {
        mutation(&cassie, &writer);
        let rows = execute(&cassie, &writer, sql);
        *published.lock().expect("warm artifact oracle") = Some(rows);
        observed.store(true, Ordering::SeqCst);
    });
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    let warm = warm
        .lock()
        .expect("warm oracle")
        .take()
        .expect("publisher read committed artifact");
    assert!(committed.load(Ordering::SeqCst));
    assert_ne!(original, warm, "publisher oracle differs after commit");
    assert_eq!(
        fresh, warm,
        "next statement sees the committed warmed artifact"
    );
    assert_eq!(
        captured, original,
        "artifact candidates, values and scores use captured Data"
    );
}

#[test]
fn should_keep_scalar_index_candidates_on_the_captured_committed_generation() {
    // Arrange
    let setup = ["CREATE INDEX native_n ON native_rows USING btree (n)"];
    let sql = "SELECT n FROM native_rows WHERE n >= 0 ORDER BY n";
    // Act
    assert_commit_artifact_view(&setup, sql);
    // Assert
    // The helper compares captured, warmed committed and next-statement oracles.
}
#[test]
fn should_keep_column_summaries_on_the_captured_committed_generation() {
    // Arrange
    let setup =
        ["CREATE INDEX native_column ON native_rows USING column (n) WITH (segment_size = 2)"];
    let sql = "SELECT SUM(n) AS total FROM native_rows";
    // Act
    assert_commit_artifact_view(&setup, sql);
    // Assert
    // The helper compares captured, warmed committed and next-statement oracles.
}
#[test]
fn should_keep_persisted_fulltext_scores_on_the_captured_committed_generation() {
    // Arrange
    let setup = ["CREATE INDEX native_text ON native_rows USING fulltext (body)"];
    let sql = "SELECT _id, search_score(body, 'alpha') AS score FROM native_rows WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 10";
    // Act
    assert_commit_artifact_view(&setup, sql);
    // Assert
    // The helper compares captured, warmed committed and next-statement oracles.
}
#[test]
fn should_keep_vector_artifacts_on_the_captured_committed_generation() {
    // Arrange
    let setup = ["CREATE INDEX native_vector ON native_rows USING vector (embedding) WITH (source_field = body, index_type = hnsw, metric = l2)"];
    let sql = "SELECT _id, vector_distance(embedding, '[1,0]') AS distance FROM native_rows ORDER BY distance ASC LIMIT 10";
    // Act
    assert_commit_artifact_view(&setup, sql);
    // Assert
    // The helper compares captured, warmed committed and next-statement oracles.
}
#[test]
fn should_keep_hybrid_artifacts_on_the_captured_committed_generation() {
    // Arrange
    let setup = ["CREATE INDEX native_text ON native_rows USING fulltext (body)", "CREATE INDEX native_vector ON native_rows USING vector (embedding) WITH (source_field = body, index_type = hnsw, metric = cosine)"];
    let sql = "SELECT _id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0]')) AS score FROM native_rows ORDER BY score DESC LIMIT 10";
    // Act
    assert_commit_artifact_view(&setup, sql);
    // Assert
    // The helper compares captured, warmed committed and next-statement oracles.
}
#[test]
fn should_keep_analytical_projection_rows_on_the_captured_committed_generation() {
    // Arrange
    let setup = ["CREATE MATERIALIZED PROJECTION native_projection WITH (analytical = true) AS SELECT id, n FROM native_rows"];
    let sql = "SELECT id, n FROM native_rows WHERE n >= 0";
    // Act
    assert_commit_artifact_view(&setup, sql);
    // Assert
    // The helper compares captured, warmed committed and next-statement oracles.
}

#[test]
fn should_keep_an_old_scalar_candidate_after_its_live_index_membership_changes() {
    // Arrange
    let setup = ["CREATE INDEX native_n ON native_rows USING btree (n)"];
    let sql = "SELECT n FROM native_rows WHERE n >= 0 AND n < 100";
    // Act
    assert_artifact_view_after_mutation(&setup, sql, |cassie, writer| {
        execute(
            cassie,
            writer,
            "UPDATE native_rows SET n = 110 WHERE id = 1",
        );
    });
    // Assert
    // The captured indexed row remains while the warmed current index excludes it.
}
#[test]
fn should_keep_an_old_fulltext_posting_after_its_live_token_changes() {
    // Arrange
    let setup = ["CREATE INDEX native_text ON native_rows USING fulltext (body)"];
    let sql = "SELECT _id, search_score(body, 'alpha') AS score FROM native_rows WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 10";
    // Act
    assert_artifact_view_after_mutation(&setup, sql, |cassie, writer| {
        execute(
            cassie,
            writer,
            "UPDATE native_rows SET body = 'beta' WHERE id = 1",
        );
    });
    // Assert
    // The captured alpha posting remains while the warmed current posting is absent.
}
#[test]
fn should_keep_the_old_vector_distance_after_its_live_embedding_changes() {
    // Arrange
    let setup = ["CREATE INDEX native_vector ON native_rows USING vector (embedding) WITH (source_field = body, index_type = hnsw, metric = l2)"];
    let sql = "SELECT _id, vector_distance(embedding, '[1,0]') AS distance FROM native_rows ORDER BY distance ASC LIMIT 10";
    // Act
    assert_artifact_view_after_mutation(&setup, sql, |cassie, writer| {
        cassie
            .execute_sql(
                writer,
                "UPDATE native_rows SET embedding = $1 WHERE id = 1",
                vec![Value::Vector(crate::types::Vector::new(vec![10.0, 0.0]))],
            )
            .expect("commit changed indexed embedding");
    });
    // Assert
    // The captured distance remains zero while the warmed current distance is nine.
}

#[test]
fn should_keep_time_series_buckets_on_the_captured_committed_generation() {
    // Arrange
    let fixture = Fixture::new(&[]);
    let reader = fixture.cassie.create_session("time-reader", None);
    let writer = fixture.cassie.create_session("time-writer", None);
    execute(
        &fixture.cassie,
        &writer,
        "CREATE TABLE native_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
    );
    execute(
        &fixture.cassie,
        &writer,
        "INSERT INTO native_events VALUES ('acme', '2026-01-01T00:00:00Z', 10)",
    );
    execute(&fixture.cassie, &writer, "CREATE INDEX native_events_time ON native_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)");
    let sql = "SELECT amount FROM native_events WHERE tenant = 'acme' AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-01T01:00:00Z' ORDER BY amount";
    let original = execute(&fixture.cassie, &reader, sql);
    let cassie = Arc::clone(&fixture.cassie);
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    let hook = CaptureHook::install(move || {
        execute(
            &cassie,
            &writer,
            "INSERT INTO native_events VALUES ('acme', '2026-01-01T00:01:00Z', 20)",
        );
        assert_eq!(
            execute(&cassie, &writer, sql),
            vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]
        );
        observed.store(true, Ordering::SeqCst);
    });
    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    let metrics = fixture.cassie.metrics();
    // Assert
    assert!(committed.load(Ordering::SeqCst));
    assert_eq!(original, vec![vec![Value::Int64(10)]]);
    assert_eq!(captured, original);
    assert_eq!(fresh, vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]);
    assert!(
        metrics["time_series"]["bucket_native_hits"]
            .as_u64()
            .unwrap_or_default()
            >= 4
    );
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}
