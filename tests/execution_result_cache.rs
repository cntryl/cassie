use cassie::types::Value;
use cassie::{Cassie, CassieRuntimeConfig};
use std::sync::{Arc, Barrier};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-execution-result-cache-{label}-{}",
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn cache_config(max_entries: usize, max_bytes: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_max_entries = max_entries;
    config.limits.execution_result_cache_max_bytes = max_bytes;
    config
}

#[test]
fn should_isolate_current_user_results() {
    // Arrange
    with_fallback();
    let path = data_dir("current-user");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let alice = cassie.create_session("alice", None);
    let bob = cassie.create_session("bob", None);
    cassie
        .execute_sql(&alice, "CREATE TABLE cache_users (marker TEXT)", vec![])
        .expect("create table");
    cassie
        .execute_sql(
            &alice,
            "INSERT INTO cache_users (marker) VALUES ('one')",
            vec![],
        )
        .expect("seed row");

    // Act
    let alice_result = cassie
        .execute_sql(
            &alice,
            "SELECT marker, current_user() FROM cache_users",
            vec![],
        )
        .expect("alice query");
    let bob_result = cassie
        .execute_sql(
            &bob,
            "SELECT marker, current_user() FROM cache_users",
            vec![],
        )
        .expect("bob query");

    // Assert
    assert_eq!(
        alice_result.rows,
        vec![vec![
            Value::String("one".into()),
            Value::String("alice".into())
        ]]
    );
    assert_eq!(
        bob_result.rows,
        vec![vec![
            Value::String("one".into()),
            Value::String("bob".into())
        ]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_cached_results_during_transaction() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let session = cassie.create_session("alice", None);
    cassie
        .execute_sql(&session, "CREATE TABLE cache_tx_docs (title TEXT)", vec![])
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO cache_tx_docs (title) VALUES ('before')",
            vec![],
        )
        .expect("seed row");
    let _ = cassie
        .execute_sql(
            &session,
            "SELECT title FROM cache_tx_docs ORDER BY title",
            vec![],
        )
        .expect("warm result cache");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO cache_tx_docs (title) VALUES ('during')",
            vec![],
        )
        .expect("stage row");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT title FROM cache_tx_docs ORDER BY title",
            vec![],
        )
        .expect("transaction query");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![Value::String("before".into())],
            vec![Value::String("during".into())],
        ]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_non_immutable_user_functions() {
    // Arrange
    with_fallback();
    let path = data_dir("stable-udf");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let session = cassie.create_session("alice", None);
    cassie
        .execute_sql(
            &session,
            r#"CREATE FUNCTION stable_echo(x TEXT) RETURNS TEXT STABLE AS "x""#,
            vec![],
        )
        .expect("create function");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT stable_echo('value')", vec![])
        .expect("first query");
    let second = cassie
        .execute_sql(&session, "SELECT stable_echo('value')", vec![])
        .expect("second query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(first.rows, second.rows);
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
        Some(2)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_volatile_user_functions() {
    // Arrange
    with_fallback();
    let path = data_dir("volatile-udf");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let session = cassie.create_session("alice", None);
    cassie
        .execute_sql(
            &session,
            r#"CREATE FUNCTION volatile_echo(x TEXT) RETURNS TEXT VOLATILE AS "x""#,
            vec![],
        )
        .expect("create function");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT volatile_echo('value')", vec![])
        .expect("first query");
    let second = cassie
        .execute_sql(&session, "SELECT volatile_echo('value')", vec![])
        .expect("second query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(first.rows, second.rows);
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
        Some(2)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cache_immutable_user_functions() {
    // Arrange
    with_fallback();
    let path = data_dir("immutable-udf");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let session = cassie.create_session("alice", None);
    cassie
        .execute_sql(
            &session,
            r#"CREATE FUNCTION immutable_echo(x TEXT) RETURNS TEXT IMMUTABLE AS "x""#,
            vec![],
        )
        .expect("create function");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT immutable_echo('value')", vec![])
        .expect("first query");
    let second = cassie
        .execute_sql(&session, "SELECT immutable_echo('value')", vec![])
        .expect("second query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(first.rows, second.rows);
    assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(1));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_observe_savepoint_rollback_without_cache() {
    // Arrange
    with_fallback();
    let path = data_dir("savepoint");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let session = cassie.create_session("alice", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE cache_savepoint (marker TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO cache_savepoint (marker) VALUES ('before')",
            vec![],
        )
        .expect("seed row");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(&session, "SAVEPOINT cache_point", vec![])
        .expect("savepoint");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO cache_savepoint (marker) VALUES ('after')",
            vec![],
        )
        .expect("stage row");
    let before_rollback = cassie
        .execute_sql(
            &session,
            "SELECT marker FROM cache_savepoint ORDER BY marker",
            vec![],
        )
        .expect("read staged row");

    // Act
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT cache_point", vec![])
        .expect("rollback to savepoint");
    let after_rollback = cassie
        .execute_sql(
            &session,
            "SELECT marker FROM cache_savepoint ORDER BY marker",
            vec![],
        )
        .expect("read rolled back state");

    // Assert
    assert_eq!(before_rollback.rows.len(), 2);
    assert_eq!(
        after_rollback.rows,
        vec![vec![Value::String("before".into())]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_virtual_catalog_results() {
    // Arrange
    with_fallback();
    let path = data_dir("virtual-catalog");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("alice", None);

    // Act
    let _ = cassie
        .execute_sql(&session, "SELECT rolname FROM pg_catalog.pg_roles", vec![])
        .expect("first catalog query");
    let _ = cassie
        .execute_sql(&session, "SELECT rolname FROM pg_catalog.pg_roles", vec![])
        .expect("second catalog query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["virtual_catalog"].as_u64(),
        Some(2)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_oversized_cache_entries() {
    // Arrange
    with_fallback();
    let path = data_dir("oversized");
    let cassie = Cassie::new_with_data_dir_and_config(&path, cache_config(64, 1)).expect("cassie");
    let session = cassie.create_session("alice", None);

    // Act
    let _ = cassie
        .execute_sql(&session, "SELECT 'payload'", vec![])
        .expect("query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(
        metrics["execution_result_cache"]["entries"].as_u64(),
        Some(0)
    );
    assert_eq!(metrics["execution_result_cache"]["bytes"].as_u64(), Some(0));
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["oversized_entry"].as_u64(),
        Some(1)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_evict_least_recently_used_result() {
    // Arrange
    with_fallback();
    let path = data_dir("lru");
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, cache_config(2, 1_000_000)).expect("cassie");
    let session = cassie.create_session("alice", None);
    cassie
        .execute_sql(&session, "CREATE TABLE cache_lru (marker TEXT)", vec![])
        .expect("create table");
    for marker in ["a", "b", "c"] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cache_lru (marker) VALUES ($1)",
                vec![Value::String(marker.into())],
            )
            .expect("insert row");
    }
    let query = "SELECT marker FROM cache_lru WHERE marker = $1";
    for marker in ["a", "b", "a", "c", "b"] {
        cassie
            .execute_sql(&session, query, vec![Value::String(marker.into())])
            .expect("cached query");
    }

    // Act
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(1));
    assert_eq!(
        metrics["execution_result_cache"]["misses"].as_u64(),
        Some(4)
    );
    assert_eq!(
        metrics["execution_result_cache"]["evictions"].as_u64(),
        Some(2)
    );
    assert_eq!(
        metrics["execution_result_cache"]["entries"].as_u64(),
        Some(2)
    );
    assert!(metrics["execution_result_cache"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_evict_results_over_byte_budget() {
    // Arrange
    with_fallback();
    let path = data_dir("byte-budget");
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, cache_config(64, 220)).expect("cassie");
    let session = cassie.create_session("alice", None);

    // Act
    cassie
        .execute_sql(&session, "SELECT 'first-payload'", vec![])
        .expect("first query");
    cassie
        .execute_sql(&session, "SELECT 'second-payload'", vec![])
        .expect("second query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(
        metrics["execution_result_cache"]["entries"].as_u64(),
        Some(1)
    );
    assert_eq!(
        metrics["execution_result_cache"]["evictions"].as_u64(),
        Some(1)
    );
    assert!(metrics["execution_result_cache"]["bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes <= 220));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_remain_fresh_during_concurrent_invalidation() {
    // Arrange
    with_fallback();
    let path = data_dir("concurrent-invalidation");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE cache_concurrent (marker TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO cache_concurrent (marker) VALUES ('before')",
            vec![],
        )
        .expect("seed row");
    let query = "SELECT marker FROM cache_concurrent ORDER BY marker";
    cassie
        .execute_sql(&setup, query, vec![])
        .expect("warm cache");
    let barrier = Arc::new(Barrier::new(2));

    // Act
    std::thread::scope(|scope| {
        let reader_cassie = Arc::clone(&cassie);
        let reader_barrier = Arc::clone(&barrier);
        scope.spawn(move || {
            let reader = reader_cassie.create_session("reader", None);
            reader_barrier.wait();
            for _ in 0..20 {
                reader_cassie
                    .execute_sql(&reader, query, vec![])
                    .expect("concurrent read");
            }
        });
        let writer_cassie = Arc::clone(&cassie);
        let writer_barrier = Arc::clone(&barrier);
        scope.spawn(move || {
            let writer = writer_cassie.create_session("writer", None);
            writer_barrier.wait();
            writer_cassie
                .execute_sql(
                    &writer,
                    "INSERT INTO cache_concurrent (marker) VALUES ('after')",
                    vec![],
                )
                .expect("concurrent write");
        });
    });
    let final_result = cassie
        .execute_sql(&setup, query, vec![])
        .expect("fresh final read");

    // Assert
    assert_eq!(
        final_result.rows,
        vec![
            vec![Value::String("after".into())],
            vec![Value::String("before".into())],
        ]
    );

    let _ = std::fs::remove_dir_all(path);
}
