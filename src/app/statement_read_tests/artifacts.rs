//! Captured source rows cannot use statistics from a newer committed corpus.
use super::{direct_plan, execute, CaptureHook, Cassie};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict artifact fixture cleanup");
    }
}
struct Fixture {
    cassie: Arc<Cassie>,
    _directory: Directory,
}
impl Fixture {
    fn new() -> Self {
        let directory = Directory(
            std::env::temp_dir().join(format!("cassie-artifact-view-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&directory.0).expect("create fixture directory");
        let mut config = CassieRuntimeConfig::default();
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie =
            Arc::new(Cassie::new_with_data_dir_and_config(&directory.0, config).expect("Cassie"));
        cassie.startup().expect("startup");
        Self {
            cassie,
            _directory: directory,
        }
    }
}

#[test]
fn should_keep_exact_fulltext_stats_cache_on_the_captured_epoch() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    execute(
        &fixture.cassie,
        &reader,
        "CREATE TABLE corpus_rows (id INT PRIMARY KEY, body TEXT)",
    );
    execute(
        &fixture.cassie,
        &reader,
        "INSERT INTO corpus_rows VALUES (1, 'alpha beta')",
    );
    let sql = "SELECT _id, search_score(body, 'alpha') AS score FROM corpus_rows WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1";
    let original = execute(&fixture.cassie, &reader, sql);
    let warmed = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&warmed);
    let cassie = Arc::clone(&fixture.cassie);
    let writer = cassie.create_session("writer", None);
    let hook = CaptureHook::install(move || {
        execute(&cassie, &writer, "INSERT INTO corpus_rows VALUES (2, 'beta'), (3, 'beta'), (4, 'beta'), (5, 'beta'), (6, 'beta'), (7, 'beta'), (8, 'beta'), (9, 'beta'), (10, 'beta')");
        *observed.lock().expect("warm result") = execute(&cassie, &writer, sql);
    });

    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    let warmed = warmed.lock().expect("warm result").clone();
    let metrics = fixture.cassie.metrics();
    drop(fixture);

    // Assert
    assert!(
        metrics["query_cache"]["fulltext_stats_hits"]
            .as_u64()
            .expect("cache hits")
            >= 2,
        "the exact path exercised the statistics cache"
    );
    assert_eq!(original.len(), 1);
    assert_ne!(
        warmed, original,
        "the committed corpus changed score statistics"
    );
    assert_eq!(fresh, warmed);
    assert_eq!(
        captured, original,
        "captured rows use captured corpus statistics"
    );
}

#[test]
fn should_recapture_fulltext_cache_identity_for_another_session() {
    // Arrange
    let fixture = Fixture::new();
    let first = fixture.cassie.create_session("first", None);
    let second = fixture.cassie.create_session("second", None);
    execute(
        &fixture.cassie,
        &first,
        "CREATE TABLE scoped_corpus (id INT PRIMARY KEY, body TEXT)",
    );
    execute(
        &fixture.cassie,
        &first,
        "INSERT INTO scoped_corpus VALUES (1, 'alpha beta')",
    );
    let sql = "SELECT _id, search_score(body, 'alpha') AS score FROM scoped_corpus WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1";
    let original = execute(&fixture.cassie, &first, sql);
    let physical = direct_plan(&fixture.cassie, sql);
    let controls = fixture
        .cassie
        .runtime
        .query_controls(std::time::Instant::now());
    let owner = first
        .capture_statement_read(
            &fixture.cassie.midge,
            &fixture.cassie.default_database,
            &controls,
        )
        .expect("first view");
    let supplied = controls.with_statement_read(Some(owner));
    execute(
        &fixture.cassie,
        &second,
        "INSERT INTO scoped_corpus VALUES (2, 'beta'), (3, 'beta')",
    );
    let fresh = execute(&fixture.cassie, &second, sql);

    // Act
    let actual = crate::executor::run_with_session_controls(
        &fixture.cassie,
        Some(&second),
        &physical,
        vec![],
        &supplied,
    )
    .expect("second view")
    .rows;
    drop(supplied);
    let released = controls.current_query_memory_bytes();
    drop(fixture);

    // Assert
    assert_ne!(
        fresh, original,
        "corpus statistics changed after the first capture"
    );
    assert_eq!(
        actual, fresh,
        "another session selects its own current view"
    );
    assert_eq!(released, 0);
}

#[test]
fn should_recapture_fulltext_cache_identity_for_another_engine() {
    // Arrange
    let first = Fixture::new();
    let second = Fixture::new();
    let first_session = first.cassie.create_session("first", None);
    let second_session = second.cassie.create_session("second", None);
    for (fixture, session) in [(&first, &first_session), (&second, &second_session)] {
        execute(
            &fixture.cassie,
            session,
            "CREATE TABLE engine_corpus (id INT PRIMARY KEY, body TEXT)",
        );
        execute(
            &fixture.cassie,
            session,
            "INSERT INTO engine_corpus VALUES (1, 'alpha beta')",
        );
    }
    execute(
        &second.cassie,
        &second_session,
        "INSERT INTO engine_corpus VALUES (2, 'beta'), (3, 'beta')",
    );
    let sql = "SELECT _id, search_score(body, 'alpha') AS score FROM engine_corpus WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1";
    let other = execute(&first.cassie, &first_session, sql);
    let expected = execute(&second.cassie, &second_session, sql);
    let physical = direct_plan(&second.cassie, sql);
    let controls = second
        .cassie
        .runtime
        .query_controls(std::time::Instant::now());
    let owner = first_session
        .capture_statement_read(
            &first.cassie.midge,
            &first.cassie.default_database,
            &controls,
        )
        .expect("other engine view");
    let supplied = controls.with_statement_read(Some(owner));

    // Act
    let actual = crate::executor::run_with_session_controls(
        &second.cassie,
        Some(&second_session),
        &physical,
        vec![],
        &supplied,
    )
    .expect("called engine view")
    .rows;
    drop(supplied);
    let released = controls.current_query_memory_bytes();
    drop(first);
    drop(second);

    // Assert
    assert_ne!(
        other, expected,
        "independent engines have distinct corpus rows"
    );
    assert_eq!(actual, expected);
    assert_eq!(released, 0);
}

#[test]
fn should_keep_staged_fulltext_corpus_statistics_private_to_the_captured_overlay() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    let other = fixture.cassie.create_session("other", None);
    for table in ["staged_corpus", "reference_corpus"] {
        execute(
            &fixture.cassie,
            &other,
            &format!("CREATE TABLE {table} (id INT PRIMARY KEY, body TEXT)"),
        );
        execute(
            &fixture.cassie,
            &other,
            &format!("INSERT INTO {table} VALUES (1, 'alpha beta')"),
        );
    }
    let additional = "(2, 'beta'), (3, 'beta'), (4, 'beta'), (5, 'beta'), (6, 'beta'), (7, 'beta'), (8, 'beta'), (9, 'beta'), (10, 'beta')";
    execute(
        &fixture.cassie,
        &other,
        &format!("INSERT INTO reference_corpus VALUES {additional}"),
    );
    let sql = "SELECT _id, search_score(body, 'alpha') AS score FROM staged_corpus WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1";
    let reference_sql = "SELECT _id, search_score(body, 'alpha') AS score FROM reference_corpus WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1";
    let committed = execute(&fixture.cassie, &other, sql);
    let reference = execute(&fixture.cassie, &other, reference_sql);
    execute(&fixture.cassie, &reader, "BEGIN");
    execute(
        &fixture.cassie,
        &reader,
        &format!("INSERT INTO staged_corpus VALUES {additional}"),
    );

    // Act
    let staged = execute(&fixture.cassie, &reader, sql);
    let isolated = execute(&fixture.cassie, &other, sql);
    execute(&fixture.cassie, &reader, "ROLLBACK");
    let rolled_back = execute(&fixture.cassie, &reader, sql);
    drop(fixture);

    // Assert
    assert_ne!(
        committed[0][1], reference[0][1],
        "the larger corpus changes statistics"
    );
    assert_eq!(
        staged[0][1], reference[0][1],
        "staged rows use their own captured corpus statistics"
    );
    assert_eq!(
        isolated, committed,
        "another session sees committed corpus statistics"
    );
    assert_eq!(
        rolled_back, committed,
        "rollback restores the committed corpus statistics"
    );
}
