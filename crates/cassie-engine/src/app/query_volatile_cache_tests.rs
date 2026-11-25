//! Volatile expressions must be evaluated again instead of replayed.
use super::*;
use crate::types::Value;
use std::cell::RefCell;

thread_local! {
    static BEFORE_CACHE_STORE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn before_execution_result_cache_store() {
    let callback = BEFORE_CACHE_STORE.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

struct CacheStoreHook;

impl CacheStoreHook {
    fn install(callback: impl FnOnce() + 'static) -> Self {
        BEFORE_CACHE_STORE.with(|slot| {
            assert!(slot.borrow().is_none(), "one scoped cache publication hook");
            *slot.borrow_mut() = Some(Box::new(callback));
        });
        Self
    }
}

impl Drop for CacheStoreHook {
    fn drop(&mut self) {
        BEFORE_CACHE_STORE.with(|slot| slot.borrow_mut().take());
    }
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
        .expect("memory fixture");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE volatile_cache_rows (value INT)",
            vec![],
        )
        .expect("table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO volatile_cache_rows VALUES (1)",
            vec![],
        )
        .expect("row");
    (cassie, session)
}

#[test]
fn should_not_replay_random_results_from_execution_cache() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT random() FROM volatile_cache_rows", vec![])
        .expect("first random query");
    let second = cassie
        .execute_sql(&session, "SELECT random() FROM volatile_cache_rows", vec![])
        .expect("second random query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_omitted_volatility_function_results_from_execution_cache() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            r#"CREATE FUNCTION r() RETURNS float AS "random()""#,
            vec![],
        )
        .expect("volatile-by-default function");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT r() FROM volatile_cache_rows", vec![])
        .expect("first routine query");
    let second = cassie
        .execute_sql(&session, "SELECT r() FROM volatile_cache_rows", vec![])
        .expect("second routine query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_replay_pure_select_results_from_execution_cache() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_rows", vec![])
        .expect("first pure query");
    let second = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_rows", vec![])
        .expect("second pure query");

    // Assert
    assert_eq!(first.rows(), vec![vec![Value::Int64(1)]]);
    assert_eq!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 1);
}

#[test]
fn should_bypass_result_cache_for_virtual_catalogs_inside_sql_expressions() {
    // Arrange
    let (cassie, session) = fixture();
    let run = |sql: &str| {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("select")
            .rows()
            .to_vec()
    };
    let metrics = || cassie.metrics()["execution_result_cache"].clone();
    let before = metrics();

    // Act
    let direct = run("SELECT count(*) FROM pg_catalog.pg_class");
    let after_direct = metrics();
    let scalar = run("SELECT (SELECT count(*) FROM pg_catalog.pg_class)");
    let scalar_again = run("SELECT (SELECT count(*) FROM pg_catalog.pg_class)");
    let after_scalar = metrics();
    let exists = run("SELECT 1 WHERE EXISTS (SELECT 1 FROM pg_catalog.pg_class)");
    let exists_again = run("SELECT 1 WHERE EXISTS (SELECT 1 FROM pg_catalog.pg_class)");
    let after_catalog = metrics();
    let ordinary = run("SELECT value FROM volatile_cache_rows");
    let ordinary_again = run("SELECT value FROM volatile_cache_rows");
    let after_ordinary = metrics();

    // Assert
    assert!(
        matches!(direct.as_slice(), [row] if matches!(row.as_slice(), [Value::Int64(n)] if *n > 0))
    );
    assert_eq!(scalar, direct);
    assert_eq!(scalar_again, direct);
    assert_eq!(exists, vec![vec![Value::Int64(1)]]);
    assert_eq!(exists_again, exists);
    let bypasses = |value: &serde_json::Value| {
        value["bypass_reasons"]["virtual_catalog"]
            .as_u64()
            .unwrap_or_default()
    };
    assert_eq!(bypasses(&after_direct) - bypasses(&before), 1);
    assert_eq!(bypasses(&after_scalar) - bypasses(&after_direct), 2);
    assert_eq!(bypasses(&after_catalog) - bypasses(&after_scalar), 2);
    assert_eq!(after_direct["hits"], before["hits"]);
    assert_eq!(after_scalar["hits"], before["hits"]);
    assert_eq!(after_catalog["hits"], before["hits"]);
    assert_eq!(ordinary_again, ordinary);
    assert_eq!(
        after_ordinary["hits"].as_u64().unwrap(),
        after_catalog["hits"].as_u64().unwrap() + 1
    );
}

#[test]
fn should_not_replay_volatile_results_hidden_in_scalar_subquery_or_exists() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let scalar_first = cassie
        .execute_sql(&session, "SELECT (SELECT random())", vec![])
        .expect("scalar subquery");
    let scalar_second = cassie
        .execute_sql(&session, "SELECT (SELECT random())", vec![])
        .expect("scalar subquery replay");
    let exists_first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows WHERE EXISTS (SELECT random())",
            vec![],
        )
        .expect("exists subquery");
    let exists_second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows WHERE EXISTS (SELECT random())",
            vec![],
        )
        .expect("exists subquery replay");

    // Assert
    assert_ne!(scalar_first.rows(), scalar_second.rows());
    assert_eq!(exists_first.rows(), exists_second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_hidden_in_a_random_view() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW volatile_cache_view AS SELECT random() AS value FROM volatile_cache_rows",
            vec![],
        )
        .expect("view");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_view", vec![])
        .expect("view query");
    let second = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_view", vec![])
        .expect("view replay");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_in_limit_offset_or_table_sample() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let limit_first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows LIMIT 1 OFFSET (SELECT (random() * 0)::int)",
            vec![],
        )
        .expect("limit query");
    let limit_second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows LIMIT 1 OFFSET (SELECT (random() * 0)::int)",
            vec![],
        )
        .expect("limit replay");
    let scalar_limit_first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows LIMIT (SELECT (random() * 0)::int)",
            vec![],
        )
        .expect("scalar limit query");
    let scalar_limit_second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows LIMIT (SELECT (random() * 0)::int)",
            vec![],
        )
        .expect("scalar limit replay");
    let sample_first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows TABLESAMPLE bernoulli (random() * 100)",
            vec![],
        )
        .expect("table sample query");
    let sample_second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows TABLESAMPLE bernoulli (random() * 100)",
            vec![],
        )
        .expect("table sample replay");

    // Assert
    assert_eq!(limit_first.rows(), limit_second.rows());
    assert_eq!(scalar_limit_first.rows(), scalar_limit_second.rows());
    assert_eq!(sample_first.columns(), sample_second.columns());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_hidden_in_a_cte() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(
            &session,
            "WITH volatile AS (SELECT random() AS value) SELECT value FROM volatile",
            vec![],
        )
        .expect("volatile CTE");
    let second = cassie
        .execute_sql(
            &session,
            "WITH volatile AS (SELECT random() AS value) SELECT value FROM volatile",
            vec![],
        )
        .expect("volatile CTE replay");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_hidden_in_a_set_branch() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(
            &session,
            "SELECT random() AS value UNION ALL SELECT random()",
            vec![],
        )
        .expect("volatile set branch");
    let second = cassie
        .execute_sql(
            &session,
            "SELECT random() AS value UNION ALL SELECT random()",
            vec![],
        )
        .expect("volatile set branch replay");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_hidden_in_a_view() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW volatile_cache_view AS SELECT random() AS value FROM volatile_cache_rows",
            vec![],
        )
        .expect("volatile function view");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_view", vec![])
        .expect("first view query");
    let second = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_view", vec![])
        .expect("second view query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_random_results_from_a_view_with_catalog_enum_types() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "CREATE TYPE volatile_cache_mood AS ENUM ('sad')",
            vec![],
        )
        .expect("enum type");
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW volatile_cache_enum_view AS SELECT 'sad'::volatile_cache_mood AS mood, random() AS value FROM volatile_cache_rows",
            vec![],
        )
        .expect("enum-typed volatile view");

    // Act
    let first = cassie
        .execute_sql(
            &session,
            "SELECT mood, value FROM volatile_cache_enum_view",
            vec![],
        )
        .expect("first enum view query");
    let second = cassie
        .execute_sql(
            &session,
            "SELECT mood, value FROM volatile_cache_enum_view",
            vec![],
        )
        .expect("second enum view query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}
#[test]
fn should_ignore_a_late_cached_result_after_another_session_commits() {
    fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("SQL")
            .rows()
            .to_vec()
    }

    // Arrange
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default()).unwrap(),
    );
    let reader = cassie.create_session("reader", None);
    let writer = cassie.create_session("writer", None);
    execute(&cassie, &writer, "CREATE TABLE cache_late_rows (id INT)");
    execute(&cassie, &writer, "INSERT INTO cache_late_rows VALUES (1)");
    let publisher = Arc::clone(&cassie);
    let hook = CacheStoreHook::install(move || {
        std::thread::spawn(move || {
            execute(&publisher, &writer, "BEGIN");
            execute(
                &publisher,
                &writer,
                "INSERT INTO cache_late_rows VALUES (2)",
            );
            execute(&publisher, &writer, "COMMIT");
        })
        .join()
        .expect("publish second row before first result is cached");
    });
    let query = "SELECT id FROM cache_late_rows ORDER BY id";

    // Act
    let late = execute(&cassie, &reader, query);
    drop(hook);
    let hits_after_late = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .unwrap();
    let fresh = execute(&cassie, &reader, query);
    let hits_after_fresh = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .unwrap();
    let warm = execute(&cassie, &reader, query);
    let hits_after_warm = cassie.metrics()["execution_result_cache"]["hits"]
        .as_u64()
        .unwrap();

    // Assert
    assert_eq!(late, vec![vec![Value::Int64(1)]]);
    assert_eq!(fresh, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
    assert_eq!(warm, fresh);
    assert_eq!(hits_after_fresh, hits_after_late);
    assert_eq!(hits_after_warm, hits_after_fresh + 1);
}
