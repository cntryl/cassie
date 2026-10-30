use super::support_sql as support;
use cassie::types::Value;
use cassie::{Cassie, CassieRuntimeConfig};
use std::sync::{Arc, Barrier};
use support::*;

fn cache_config(max_entries: usize, max_bytes: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_max_entries = max_entries;
    config.limits.execution_result_cache_max_bytes = max_bytes;
    config
}

#[test]
fn should_execute_session_mutations_for_each_matching_session() {
    // Arrange
    let path = data_dir("session-mutating-builtins");
    let cassie = Cassie::new_in_memory_with_config(cache_config(128, 1_048_576)).expect("cassie");
    let setup = cassie.create_session("reader", None);
    cassie
        .execute_sql(&setup, "CREATE TABLE cache_seed (marker INT)", vec![])
        .expect("table");
    cassie
        .execute_sql(&setup, "INSERT INTO cache_seed VALUES (1)", vec![])
        .expect("seed");
    cassie.execute_sql(&setup, "CREATE VIEW cache_mutation AS SELECT set_config('application_name', 'view', false) AS applied FROM cache_seed", vec![]).expect("view");
    cassie
        .execute_sql(
            &setup,
            "CREATE VIEW cache_nested_mutation AS SELECT applied FROM cache_mutation",
            vec![],
        )
        .expect("nested view");

    // Act
    for (sql, expected) in [
        (
            "SELECT set_config('application_name', 'direct', false)",
            "direct",
        ),
        (
            "SELECT pg_catalog.set_config('application_name', 'qualified', false)",
            "qualified",
        ),
        ("SELECT applied FROM cache_mutation", "view"),
        ("SELECT applied FROM cache_nested_mutation", "view"),
    ] {
        let first = cassie.create_session("reader", None);
        let second = cassie.create_session("reader", None);
        cassie
            .execute_sql(&first, sql, vec![])
            .expect("first mutation");
        let result = cassie
            .execute_sql(&second, sql, vec![])
            .expect("second mutation");

        // Assert
        assert_eq!(result.rows(), vec![vec![Value::String(expected.into())]]);
        assert_eq!(
            second.setting("application_name").expect("setting"),
            expected
        );
    }
    let hits_before = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits");
    cassie
        .execute_sql(&setup, "SELECT marker FROM cache_seed", vec![])
        .expect("warm pure query");
    cassie
        .execute_sql(&setup, "SELECT marker FROM cache_seed", vec![])
        .expect("cached pure query");
    assert_eq!(
        cassie.metrics()["execution_result_cache"]["hits"]
            .as_u64()
            .expect("cache hits"),
        hits_before + 1
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_isolate_current_user_results() {
    // Arrange
    let path = data_dir("current-user");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
        alice_result.rows(),
        vec![vec![
            Value::String("one".into()),
            Value::String("alice".into())
        ]]
    );
    assert_eq!(
        bob_result.rows(),
        vec![vec![
            Value::String("one".into()),
            Value::String("bob".into())
        ]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_isolate_execution_results_given_different_authenticated_roles() {
    // Arrange
    let path = data_dir("authenticated-roles");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    let alice = cassie.create_session("alice", None);
    let bob = cassie.create_session("bob", None);
    cassie
        .execute_sql(&alice, "CREATE TABLE cache_role_rows (value TEXT)", vec![])
        .expect("create table");
    cassie
        .execute_sql(
            &alice,
            "INSERT INTO cache_role_rows (value) VALUES ('shared')",
            vec![],
        )
        .expect("seed row");

    // Act
    let alice_result = cassie
        .execute_sql(&alice, "SELECT value FROM cache_role_rows", vec![])
        .expect("alice query");
    let bob_result = cassie
        .execute_sql(&bob, "SELECT value FROM cache_role_rows", vec![])
        .expect("bob query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(alice_result.rows(), bob_result.rows());
    assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(0));
    assert_eq!(
        metrics["execution_result_cache"]["misses"].as_u64(),
        Some(2)
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_isolate_execution_results_given_different_search_paths() {
    // Arrange
    let path = data_dir("search-paths");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    for sql in [
        "CREATE SCHEMA alpha",
        "CREATE SCHEMA beta",
        "CREATE TABLE alpha.cache_path_rows (value TEXT)",
        "CREATE TABLE beta.cache_path_rows (value TEXT)",
        "INSERT INTO alpha.cache_path_rows (value) VALUES ('alpha')",
        "INSERT INTO beta.cache_path_rows (value) VALUES ('beta')",
    ] {
        cassie
            .execute_sql(&setup, sql, vec![])
            .expect("setup schema");
    }
    let alpha = cassie.create_session("reader", None);
    let beta = cassie.create_session("reader", None);
    cassie
        .execute_sql(&alpha, "SET search_path TO alpha", vec![])
        .expect("alpha path");
    cassie
        .execute_sql(&beta, "SET search_path TO beta", vec![])
        .expect("beta path");

    // Act
    let alpha_result = cassie
        .execute_sql(&alpha, "SELECT value FROM cache_path_rows", vec![])
        .expect("alpha query");
    let beta_result = cassie
        .execute_sql(&beta, "SELECT value FROM cache_path_rows", vec![])
        .expect("beta query");

    // Assert
    assert_eq!(
        alpha_result.rows(),
        vec![vec![Value::String("alpha".into())]]
    );
    assert_eq!(beta_result.rows(), vec![vec![Value::String("beta".into())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_refresh_cached_results_after_same_session_search_path_changes() {
    // Arrange
    let path = data_dir("same-session-search-path");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    for sql in [
        "CREATE SCHEMA alpha",
        "CREATE SCHEMA beta",
        "CREATE TABLE alpha.cache_path_rows (value TEXT)",
        "CREATE TABLE beta.cache_path_rows (value TEXT)",
        "INSERT INTO alpha.cache_path_rows (value) VALUES ('alpha')",
        "INSERT INTO beta.cache_path_rows (value) VALUES ('beta')",
    ] {
        cassie
            .execute_sql(&setup, sql, vec![])
            .expect("setup schema");
    }
    let reader = cassie.create_session("reader", None);
    let query = "SELECT value FROM cache_path_rows";
    cassie
        .execute_sql(&reader, "SET search_path TO alpha", vec![])
        .expect("alpha path");
    let hits_before_alpha = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits");

    // Act
    let alpha_result = cassie
        .execute_sql(&reader, query, vec![])
        .expect("first alpha query");
    cassie
        .execute_sql(&reader, query, vec![])
        .expect("warm alpha result");
    let hits_after_alpha = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits");
    cassie
        .execute_sql(&reader, "SET search_path TO beta", vec![])
        .expect("beta path");
    let beta_result = cassie
        .execute_sql(&reader, query, vec![])
        .expect("first beta query");
    let hits_after_beta = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits");
    cassie
        .execute_sql(&reader, query, vec![])
        .expect("warm beta result");
    let hits_after_beta_warm = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .expect("cache hits");

    // Assert
    assert_eq!(
        alpha_result.rows(),
        vec![vec![Value::String("alpha".into())]]
    );
    assert_eq!(beta_result.rows(), vec![vec![Value::String("beta".into())]]);
    assert_eq!(hits_after_alpha, hits_before_alpha + 1);
    assert_eq!(hits_after_beta, hits_after_alpha);
    assert_eq!(hits_after_beta_warm, hits_after_beta + 1);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_isolate_execution_results_given_different_session_settings() {
    // Arrange
    let path = data_dir("session-settings");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    let first = cassie.create_session("reader", None);
    let second = cassie.create_session("reader", None);
    cassie
        .execute_sql(&first, "SET application_name TO first_client", vec![])
        .expect("first setting");
    cassie
        .execute_sql(&second, "SET application_name TO second_client", vec![])
        .expect("second setting");

    // Act
    let first_result = cassie
        .execute_sql(&first, "SELECT current_setting('application_name')", vec![])
        .expect("first setting query");
    let second_result = cassie
        .execute_sql(
            &second,
            "SELECT current_setting('application_name')",
            vec![],
        )
        .expect("second setting query");

    // Assert
    assert_eq!(
        first_result.rows(),
        vec![vec![Value::String("first_client".into())]]
    );
    assert_eq!(
        second_result.rows(),
        vec![vec![Value::String("second_client".into())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_cached_results_during_transaction() {
    // Arrange
    let path = data_dir("transaction");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
        result.rows(),
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
    let path = data_dir("stable-udf");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    assert_eq!(first.rows(), second.rows());
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
        Some(2)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_volatile_user_functions() {
    // Arrange
    let path = data_dir("volatile-udf");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    assert_eq!(first.rows(), second.rows());
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
        Some(2)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_volatile_user_functions_after_startup_qualifies_catalog_names() {
    // Arrange
    let path = data_dir("started-volatile-udf");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
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
    assert_eq!(first.rows(), second.rows());
    assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(0));
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
        Some(2)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_volatile_user_functions_called_with_different_identifier_case() {
    // Arrange
    let path = data_dir("volatile-udf-case");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
        .execute_sql(&session, "SELECT VOLATILE_ECHO('value')", vec![])
        .expect("first query");
    let second = cassie
        .execute_sql(&session, "SELECT VOLATILE_ECHO('value')", vec![])
        .expect("second query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(first.rows(), second.rows());
    assert_eq!(
        metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
        Some(2)
    );
    assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(0));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cache_immutable_user_functions() {
    // Arrange
    let path = data_dir("immutable-udf");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    assert_eq!(first.rows(), second.rows());
    assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(1));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_observe_savepoint_rollback_without_cache() {
    // Arrange
    let path = data_dir("savepoint");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    assert_eq!(before_rollback.rows().len(), 2);
    assert_eq!(
        after_rollback.rows(),
        vec![vec![Value::String("before".into())]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bypass_virtual_catalog_results() {
    // Arrange
    let path = data_dir("virtual-catalog");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    let path = data_dir("oversized");
    let cassie = Cassie::new_in_memory_with_config(cache_config(64, 1)).expect("cassie");
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
    let path = data_dir("lru");
    let cassie = Cassie::new_in_memory_with_config(cache_config(2, 1_000_000)).expect("cassie");
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
    let path = data_dir("byte-budget");
    let cassie = Cassie::new_in_memory_with_config(cache_config(64, 220)).expect("cassie");
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
    let path = data_dir("concurrent-invalidation");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
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
        final_result.rows(),
        vec![
            vec![Value::String("after".into())],
            vec![Value::String("before".into())],
        ]
    );

    let _ = std::fs::remove_dir_all(path);
}
