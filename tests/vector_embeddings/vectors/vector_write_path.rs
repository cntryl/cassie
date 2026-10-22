use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::types::{Value, Vector};

use super::support_sql as support;
use support::{data_dir, use_local_storage};

mod publication_restart {
    include!("../publication/restart.rs");
}

mod publication_orphans {
    include!("../publication/orphans.rs");
}

mod cache_publication {
    include!("../publication/cache.rs");
}

fn new_in_memory_vector_cassie() -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    Cassie::new_in_memory_with_config(config).expect("create in-memory Cassie")
}

fn start_in_memory_vector_cassie() -> Cassie {
    let cassie = new_in_memory_vector_cassie();
    cassie.startup().expect("start in-memory Cassie");
    cassie
}

fn new_vector_cassie(path: &str) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    Cassie::new_with_data_dir_and_config(path, config).expect("create Cassie")
}

fn start_vector_cassie(path: &str) -> Cassie {
    let cassie = new_vector_cassie(path);
    cassie.startup().expect("start Cassie");
    cassie
}

fn run(cassie: &Cassie, sql: &str, params: Vec<Value>) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    let Ok(result) = cassie.execute_sql(&session, sql, params) else {
        panic!("statement failed: {sql}");
    };
    result.rows().to_vec()
}

fn error_text(cassie: &Cassie, sql: &str, params: Vec<Value>) -> String {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, params)
        .err()
        .map(|error| error.to_string())
        .expect("statement should fail")
}

fn create_table(cassie: &Cassie, table: &str, with_index: bool) {
    run(
        cassie,
        &format!("CREATE TABLE {table} (id TEXT, body TEXT, embedding VECTOR(3))"),
        vec![],
    );
    if with_index {
        create_index(cassie, table);
    }
}

fn create_index(cassie: &Cassie, table: &str) {
    run(
            cassie,
            &format!(
                "CREATE INDEX {table}_idx ON {table} USING vector (embedding) WITH (source_field = body)"
            ),
            vec![],
        );
}

fn embedding_of(cassie: &Cassie, table: &str, id: &str) -> Value {
    let rows = run(
        cassie,
        &format!("SELECT embedding FROM {table} WHERE id = '{id}'"),
        vec![],
    );
    rows.into_iter()
        .next()
        .and_then(|row| row.into_iter().next())
        .unwrap_or(Value::Bool(false))
}

fn same(left: &Value, right: &Value) -> bool {
    left == right
}

fn vector(values: [f32; 3]) -> Value {
    Value::Vector(Vector::new(values.to_vec()))
}

#[test]
fn should_store_explicit_vector_given_insert_into_sourced_vector_column() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "explicit_insert", true);

    // Act
    run(
        &cassie,
        "INSERT INTO explicit_insert (id, body, embedding) VALUES ('a', 'gamma', $1)",
        vec![vector([1.0, 0.0, 0.0])],
    );
    run(
        &cassie,
        "INSERT INTO explicit_insert (id, body) VALUES ('b', 'gamma')",
        vec![],
    );

    // Assert
    assert!(same(
        &embedding_of(&cassie, "explicit_insert", "a"),
        &vector([1.0, 0.0, 0.0])
    ));
    let derived = embedding_of(&cassie, "explicit_insert", "b");
    assert!(!same(&derived, &vector([1.0, 0.0, 0.0])));
    assert!(matches!(derived, Value::Vector(_)));
    drop(cassie);
}

#[test]
fn should_store_explicit_vector_given_update_assigns_embedding_column() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "explicit_update", true);
    run(
        &cassie,
        "INSERT INTO explicit_update (id, body) VALUES ('a', 'gamma')",
        vec![],
    );
    let derived = embedding_of(&cassie, "explicit_update", "a");

    // Act
    let returned = run(
        &cassie,
        "UPDATE explicit_update SET embedding = $1 WHERE id = 'a' RETURNING embedding",
        vec![vector([0.0, 1.0, 0.0])],
    );

    // Assert
    assert!(!same(&derived, &vector([0.0, 1.0, 0.0])));
    assert!(same(
        &returned[0][0],
        &Value::Json(serde_json::json!([0.0, 1.0, 0.0]))
    ));
    assert!(same(
        &embedding_of(&cassie, "explicit_update", "a"),
        &vector([0.0, 1.0, 0.0])
    ));
    drop(cassie);
}

#[test]
fn should_regenerate_embedding_given_update_changes_source_column() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "regenerate", true);
    run(
        &cassie,
        "INSERT INTO regenerate (id, body) VALUES ('a', 'alpha'), ('b', 'beta')",
        vec![],
    );
    let before = embedding_of(&cassie, "regenerate", "a");
    let beta = embedding_of(&cassie, "regenerate", "b");

    // Act
    run(
        &cassie,
        "UPDATE regenerate SET body = 'beta' WHERE id = 'a'",
        vec![],
    );

    // Assert
    let after = embedding_of(&cassie, "regenerate", "a");
    assert!(!same(&after, &before));
    assert!(same(&after, &beta));
    drop(cassie);
}

#[test]
fn should_clear_embedding_given_update_sets_source_column_to_null() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "clear_source", true);
    run(
        &cassie,
        "INSERT INTO clear_source (id, body) VALUES ('a', 'hello'), ('b', 'hello'), ('c', 'null')",
        vec![],
    );
    let null_text = embedding_of(&cassie, "clear_source", "c");

    // Act
    run(
        &cassie,
        "UPDATE clear_source SET body = NULL WHERE id = 'a'",
        vec![],
    );
    run(
        &cassie,
        "UPDATE clear_source SET body = NULL, embedding = $1 WHERE id = 'b'",
        vec![vector([0.0, 0.0, 1.0])],
    );
    run(
        &cassie,
        "INSERT INTO clear_source (id, body) VALUES ('d', NULL)",
        vec![],
    );

    // Assert
    assert!(same(
        &embedding_of(&cassie, "clear_source", "a"),
        &Value::Null
    ));
    assert!(same(
        &embedding_of(&cassie, "clear_source", "b"),
        &vector([0.0, 0.0, 1.0])
    ));
    assert!(same(
        &embedding_of(&cassie, "clear_source", "d"),
        &Value::Null
    ));
    assert!(matches!(null_text, Value::Vector(_)));
    drop(cassie);
}

#[test]
fn should_embed_existing_rows_given_vector_index_created_on_populated_table() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "backfill", false);
    run(
        &cassie,
        "INSERT INTO backfill (id, body) VALUES ('a', 'aa'), ('b', 'bb'), ('n', NULL)",
        vec![],
    );
    run(
        &cassie,
        "INSERT INTO backfill (id, body, embedding) VALUES ('e', 'ee', $1)",
        vec![vector([0.0, 0.0, 1.0])],
    );

    // Act
    create_index(&cassie, "backfill");
    run(
        &cassie,
        "INSERT INTO backfill (id, body) VALUES ('ref', 'aa')",
        vec![],
    );

    // Assert
    let reference = embedding_of(&cassie, "backfill", "ref");
    assert!(matches!(reference, Value::Vector(_)));
    assert!(same(&embedding_of(&cassie, "backfill", "a"), &reference));
    assert!(matches!(
        embedding_of(&cassie, "backfill", "b"),
        Value::Vector(_)
    ));
    assert!(same(&embedding_of(&cassie, "backfill", "n"), &Value::Null));
    assert!(same(
        &embedding_of(&cassie, "backfill", "e"),
        &vector([0.0, 0.0, 1.0])
    ));
    drop(cassie);
}

#[test]
fn should_accept_vector_string_literal_given_insert_or_update() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "literal", false);

    // Act
    run(
        &cassie,
        "INSERT INTO literal (id, embedding) VALUES ('a', '[1,2,3]')",
        vec![],
    );
    run(
        &cassie,
        "INSERT INTO literal (id, embedding) VALUES ($1, $2)",
        vec![
            Value::String("b".to_string()),
            Value::String("[4,5,6]".to_string()),
        ],
    );
    run(
        &cassie,
        "INSERT INTO literal (id, embedding) VALUES ('c', '[0,0,0]')",
        vec![],
    );
    run(
        &cassie,
        "UPDATE literal SET embedding = '[7,8,9]' WHERE id = 'c'",
        vec![],
    );

    // Assert
    assert!(same(
        &embedding_of(&cassie, "literal", "a"),
        &vector([1.0, 2.0, 3.0])
    ));
    assert!(same(
        &embedding_of(&cassie, "literal", "b"),
        &vector([4.0, 5.0, 6.0])
    ));
    assert!(same(
        &embedding_of(&cassie, "literal", "c"),
        &vector([7.0, 8.0, 9.0])
    ));
    drop(cassie);
}

#[test]
fn should_report_range_error_given_vector_literal_component_exceeds_f32() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "literal_range", false);
    run(
        &cassie,
        "INSERT INTO literal_range (id, embedding) VALUES ('a', '[1,2,3]')",
        vec![],
    );

    // Act
    let update = error_text(
        &cassie,
        "UPDATE literal_range SET embedding = '[1e39,0,0]' WHERE id = 'a'",
        vec![],
    );
    let insert = error_text(
        &cassie,
        "INSERT INTO literal_range (id, embedding) VALUES ('b', '[1e39,0,0]')",
        vec![],
    );

    // Assert
    assert!(update.contains("outside f32 range"));
    assert!(insert.contains("outside f32 range"));
    assert!(same(
        &embedding_of(&cassie, "literal_range", "a"),
        &vector([1.0, 2.0, 3.0])
    ));
    drop(cassie);
}

#[test]
fn should_preserve_constraints_before_vector_backfill_publication() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    run(&cassie, "CREATE TABLE guarded (id TEXT, body TEXT, embedding VECTOR(3), CHECK (embedding = 'forbidden'))", vec![]);
    run(
        &cassie,
        "INSERT INTO guarded (id, body) VALUES ('a', 'hello')",
        vec![],
    );

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie.execute_sql(
        &session,
        "CREATE INDEX guarded_idx ON guarded USING vector (embedding) WITH (source_field = body)",
        vec![],
    );

    // Assert
    assert!(result.is_err());
    assert!(matches!(embedding_of(&cassie, "guarded", "a"), Value::Null));
    assert!(cassie
        .midge
        .get_vector_index("guarded", "embedding")
        .expect("index metadata")
        .is_none());
    drop(cassie);
}

#[test]
fn should_leave_rows_unchanged_after_vector_backfill_provider_failure() {
    // Arrange
    let mut cassie = start_in_memory_vector_cassie();
    cassie.embedding_provider =
        std::sync::Arc::new(super::backfill_provider::FailSecondProvider::default());
    create_table(&cassie, "partial_backfill", false);
    run(
        &cassie,
        "INSERT INTO partial_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie.execute_sql(&session, "CREATE INDEX partial_idx ON partial_backfill USING vector (embedding) WITH (source_field = body)", vec![]);

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "partial_backfill", "a"),
        Value::Null
    ));
    assert!(matches!(
        embedding_of(&cassie, "partial_backfill", "b"),
        Value::Null
    ));
    assert!(cassie
        .midge
        .get_vector_index("partial_backfill", "embedding")
        .expect("index metadata")
        .is_none());
    drop(cassie);
}

fn rest_index(cassie: &Cassie, table: &str) -> Result<serde_json::Value, cassie::app::CassieError> {
    cassie::rest::indexes::create(
        cassie,
        table,
        br#"{"field":"embedding","options":{"source_field":"body"}}"#,
    )
}

#[test]
fn should_preserve_constraints_before_rest_vector_backfill_publication() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    run(&cassie, "CREATE TABLE guarded (id TEXT, body TEXT, embedding VECTOR(3), CHECK (embedding = 'forbidden'))", vec![]);
    run(
        &cassie,
        "INSERT INTO guarded (id, body) VALUES ('a', 'hello')",
        vec![],
    );

    // Act
    let result = rest_index(&cassie, "guarded");

    // Assert
    assert!(result.is_err());
    assert!(matches!(embedding_of(&cassie, "guarded", "a"), Value::Null));
    drop(cassie);
}

#[test]
fn should_leave_rows_unchanged_after_rest_vector_backfill_provider_failure() {
    // Arrange
    let mut cassie = start_in_memory_vector_cassie();
    cassie.embedding_provider =
        std::sync::Arc::new(super::backfill_provider::FailSecondProvider::default());
    create_table(&cassie, "partial_backfill", false);
    run(
        &cassie,
        "INSERT INTO partial_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );

    // Act
    let result = rest_index(&cassie, "partial_backfill");

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "partial_backfill", "a"),
        Value::Null
    ));
    assert!(matches!(
        embedding_of(&cassie, "partial_backfill", "b"),
        Value::Null
    ));
    drop(cassie);
}

fn reopen_without_provider_calls(path: &str) -> Cassie {
    let mut cassie = new_vector_cassie(path);
    let provider = std::sync::Arc::new(super::backfill_provider::FailSecondProvider::default());
    cassie.embedding_provider = provider.clone();
    cassie.startup().expect("recover publication");
    assert_eq!(provider.call_count(), 0);
    cassie
}

fn assert_recovered_backfill(cassie: &Cassie) {
    assert!(matches!(
        embedding_of(cassie, "recover_backfill", "a"),
        Value::Vector(_)
    ));
    assert!(matches!(
        embedding_of(cassie, "recover_backfill", "b"),
        Value::Vector(_)
    ));
    assert!(cassie
        .midge
        .get_vector_index("recover_backfill", "embedding")
        .expect("vector metadata")
        .is_some());
    assert!(cassie
        .catalog
        .get_vector_index("postgres.public.recover_backfill", "embedding")
        .is_some());
    cassie
        .midge
        .replay_pending_index_publications()
        .expect("repeat replay");
}

#[test]
fn should_replay_unapplied_vector_backfill_after_publication_interruption() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_backfill_prepared_restart");
    let cassie = start_vector_cassie(&path);
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );
    cassie::midge::adapter::set_index_publication_failure_point(true);

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie.execute_sql(&session, "CREATE INDEX recover_idx ON recover_backfill USING vector (embedding) WITH (source_field = body)", vec![]);
    cassie::midge::adapter::set_index_publication_failure_point(false);

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "a"),
        Value::Null
    ));
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "b"),
        Value::Null
    ));
    drop(cassie);
    let cassie = reopen_without_provider_calls(&path);
    assert_recovered_backfill(&cassie);
    assert!(cassie
        .midge
        .get_index("recover_backfill", "recover_idx")
        .expect("SQL index")
        .is_some());
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_atomic_vector_backfill_after_data_commit_failure() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_backfill_atomic_restart");
    let cassie = start_vector_cassie(&path);
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );
    cassie::midge::adapter::set_document_write_storage_failures(
        cassie::midge::adapter::DocumentWriteStorageFailure::Fenced,
        1,
    );

    // Act
    let session = cassie.create_session("tester", None);
    let result = cassie.execute_sql(&session, "CREATE INDEX recover_idx ON recover_backfill USING vector (embedding) WITH (source_field = body)", vec![]);

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "a"),
        Value::Null
    ));
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "b"),
        Value::Null
    ));
    drop(cassie);
    let cassie = reopen_without_provider_calls(&path);
    assert_recovered_backfill(&cassie);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_committed_rest_vector_backfill_after_sidecar_failure() {
    // Arrange
    use_local_storage();
    let path = data_dir("rest_vector_backfill_committed_restart");
    let cassie = start_vector_cassie(&path);
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "a"),
        Value::Null
    ));
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "b"),
        Value::Null
    ));
    cassie::midge::adapter::set_document_write_failure_point(Some(
        cassie::midge::adapter::DocumentWriteFailurePoint::VectorState,
    ));

    // Act
    let result = rest_index(&cassie, "recover_backfill");
    cassie::midge::adapter::set_document_write_failure_point(None);

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "a"),
        Value::Vector(_)
    ));
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "b"),
        Value::Vector(_)
    ));
    assert!(cassie
        .midge
        .get_vector_index("recover_backfill", "embedding")
        .expect("metadata")
        .is_none());
    drop(cassie);
    let cassie = reopen_without_provider_calls(&path);
    assert_recovered_backfill(&cassie);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_expose_rollup_debt_after_committed_backfill_failure() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    run(
        &cassie,
        "CREATE TABLE recover_backfill (id TEXT, body TEXT, event_at TEXT, embedding VECTOR(3))",
        vec![],
    );
    run(&cassie, "INSERT INTO recover_backfill (id, body, event_at) VALUES ('a', 'hello', '2026-01-01T00:05:00Z')", vec![]);
    run(&cassie, "CREATE ROLLUP backfill_hourly ON recover_backfill USING time_bucket('1 hour', event_at) GROUP BY body AGGREGATES COUNT(*) AS total", vec![]);
    let explain_sql = "EXPLAIN SELECT time_bucket('1 hour', event_at) AS bucket, body, COUNT(*) AS total FROM recover_backfill GROUP BY time_bucket('1 hour', event_at), body";
    let before = run(&cassie, explain_sql, vec![]);
    assert!(before
        .iter()
        .flatten()
        .any(|value| value.as_str().is_some_and(
            |plan| plan.contains("rollup_rewrite=") && !plan.contains("rollup_rewrite=none")
        )));
    cassie::midge::adapter::set_document_write_failure_point(Some(
        cassie::midge::adapter::DocumentWriteFailurePoint::VectorState,
    ));

    // Act
    let result = rest_index(&cassie, "recover_backfill");
    cassie::midge::adapter::set_document_write_failure_point(None);
    let debt = run(&cassie, "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.recover_backfill' AND artifact = 'rollup'", vec![]);

    let after = run(&cassie, explain_sql, vec![]);

    // Assert
    assert!(result.is_err());
    assert!(after.iter().flatten().any(|value| value
        .as_str()
        .is_some_and(|plan| plan.contains("rollup_rewrite=none"))));
    assert_eq!(debt.len(), 1);
    drop(cassie);
}

fn interrupt_before_backfill(cassie: &Cassie) {
    cassie::midge::adapter::set_index_publication_failure_point(true);
    let session = cassie.create_session("tester", None);
    let result = cassie.execute_sql(&session, "CREATE INDEX recover_idx ON recover_backfill USING vector (embedding) WITH (source_field = body)", vec![]);
    cassie::midge::adapter::set_index_publication_failure_point(false);
    assert!(result.is_err());
}

#[test]
fn should_fail_closed_on_unknown_vector_publication_version() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_backfill_unknown_version");
    let cassie = start_vector_cassie(&path);
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello')",
        vec![],
    );
    interrupt_before_backfill(&cassie);
    let (key, mut record) = super::vector_publication_support::pending_record(&cassie);
    record["vector_backfill"]["version"] = serde_json::json!(99);
    super::vector_publication_support::write_database_record(&cassie, key, Some(&record));
    drop(cassie);

    // Act
    let cassie = new_vector_cassie(&path);
    let result = cassie.startup();

    // Assert
    assert!(result.is_err());
    assert!(cassie
        .midge
        .get_vector_index("recover_backfill", "embedding")
        .expect("metadata")
        .is_none());
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fail_closed_on_missing_vector_backfill_staging_row() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );
    interrupt_before_backfill(&cassie);
    let (key, _) = super::vector_publication_support::staged_record(&cassie);
    super::vector_publication_support::write_database_record(&cassie, key, None);

    // Act
    let result = cassie.midge.replay_pending_index_publications();

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "a"),
        Value::Null
    ));
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "b"),
        Value::Null
    ));
    drop(cassie);
}

#[test]
fn should_fail_closed_on_changed_vector_backfill_staging_digest() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello')",
        vec![],
    );
    interrupt_before_backfill(&cassie);
    let (key, mut record) = super::vector_publication_support::staged_record(&cassie);
    record["payload"]["body"] = serde_json::json!("corrupted");
    super::vector_publication_support::write_database_record(&cassie, key, Some(&record));

    // Act
    let result = cassie.midge.replay_pending_index_publications();

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "a"),
        Value::Null
    ));
    drop(cassie);
}

#[test]
fn should_preserve_newer_source_rows_when_vector_publication_generation_changes() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello')",
        vec![],
    );
    interrupt_before_backfill(&cassie);
    run(
        &cassie,
        "UPDATE recover_backfill SET body = 'newer' WHERE id = 'a'",
        vec![],
    );

    // Act
    let result = cassie.midge.replay_pending_index_publications();

    // Assert
    assert!(result.is_err());
    assert!(matches!(
        embedding_of(&cassie, "recover_backfill", "a"),
        Value::Null
    ));
    let body = run(
        &cassie,
        "SELECT body FROM recover_backfill WHERE id = 'a'",
        vec![],
    );
    assert!(matches!(&body[0][0], Value::String(value) if value == "newer"));
    drop(cassie);
}

#[test]
fn should_backfill_unique_rows_without_conflicting_with_their_own_identity() {
    // Arrange
    let cassie = start_in_memory_vector_cassie();
    run(
        &cassie,
        "CREATE TABLE recover_backfill (id TEXT UNIQUE, body TEXT, embedding VECTOR(3))",
        vec![],
    );
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );

    // Act
    create_index(&cassie, "recover_backfill");

    // Assert
    assert_recovered_backfill(&cassie);
    drop(cassie);
}
