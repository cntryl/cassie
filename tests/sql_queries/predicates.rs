#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support_sql as support;
use support::*;

#[test]
fn should_filter_rows_with_is_null_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_is_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_is_null (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_null (title, archived_at) VALUES ('alpha', NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_null (title, archived_at) VALUES ('beta', 'today')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_is_null WHERE archived_at IS NULL",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_in_list_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_in_list");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_in_list (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_in_list (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_in_list (title) VALUES ('gamma')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_in_list WHERE title IN ('alpha', 'beta')",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_in_list_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_not_in_list");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_in_list (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_in_list (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_in_list (title) VALUES ('gamma')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_in_list WHERE title NOT IN ('alpha', 'beta')",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("gamma".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_between_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_between");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_between (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_between (title, score) VALUES ('alpha', 5)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_between (title, score) VALUES ('beta', 15)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_between WHERE score BETWEEN 10 AND 20",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_between_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_not_between");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_between (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_between (title, score) VALUES ('alpha', 5)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_between (title, score) VALUES ('beta', 15)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_between WHERE score NOT BETWEEN 10 AND 20",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_nulls_first_when_requested() {
    // Arrange
    use_local_storage();
    let path = data_dir("order_nulls_first");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE order_nulls_first (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_first (title, archived_at) VALUES ('alpha', 'today')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_first (title, archived_at) VALUES ('beta', NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM order_nulls_first ORDER BY archived_at NULLS FIRST",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("beta".to_string())],
                vec![Value::String("alpha".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_nulls_last_when_requested() {
    // Arrange
    use_local_storage();
    let path = data_dir("order_nulls_last");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE order_nulls_last (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_last (title, archived_at) VALUES ('alpha', 'today')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_last (title, archived_at) VALUES ('beta', NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM order_nulls_last ORDER BY archived_at NULLS LAST",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_exists_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_outer (title TEXT)", vec![])

.unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_inner (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_inner (title) VALUES ('present')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_exists_outer WHERE EXISTS (SELECT title FROM predicate_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_empty_exists_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_empty_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_empty_exists_outer (title TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_empty_exists_inner (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_empty_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_empty_exists_outer WHERE EXISTS (SELECT title FROM predicate_empty_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, [] as [std::vec::Vec<cassie::types::Value>; 0]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_not");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_docs (title) VALUES ('keep')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_docs (title) VALUES ('skip')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_docs WHERE NOT title = 'skip'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("keep".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_exists_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_not_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_exists_outer (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_exists_inner (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_exists_outer WHERE NOT EXISTS (SELECT title FROM predicate_not_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_is_not_null_predicate() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_is_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_is_not_null (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('alpha', NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('beta', 'today')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_is_not_null WHERE archived_at IS NOT NULL",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_match_like_wildcards_anywhere_in_pattern() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_like_wildcards");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE predicate_like_wildcards (row_key INT, name TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_like_wildcards (row_key, name) VALUES (1, 'foobar'), (2, 'abc'), (3, 'ABC'), (4, 'a%c'), (5, 'a_c'), (6, 'axxc'), (7, 'fooxbarbar'), (8, NULL)",
                vec![],
            )
            .expect("seed rows");
    let selected_ids = |predicate: &str, params: Vec<Value>| {
        cassie
                .execute_sql(
                    &session,
                    &format!(
                        "SELECT row_key FROM predicate_like_wildcards WHERE {predicate} ORDER BY row_key"
                    ),
                    params,
                )
                .expect("evaluate like predicate")
                .rows
                .into_iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>()
    };
    let ids = |values: &[i64]| values.iter().copied().map(Value::Int64).collect::<Vec<_>>();

    // Act
    let interior = selected_ids("name LIKE 'foo%bar'", vec![]);
    let multiple = selected_ids("name LIKE '%a%c%'", vec![]);
    let single = selected_ids("name LIKE 'a_c'", vec![]);
    let repeated = selected_ids("name LIKE 'foo%%bar'", vec![]);
    let escaped_percent = selected_ids("name LIKE 'a\\%c'", vec![]);
    let escaped_underscore = selected_ids("name LIKE 'a\\_c'", vec![]);
    let case_sensitive = selected_ids("name LIKE 'A%'", vec![]);
    let parameterized = selected_ids("name LIKE $1", vec![Value::String("%o%b_r".to_string())]);
    let negated = selected_ids("NOT (name LIKE '%a%')", vec![]);

    // Assert
    assert_eq!(interior, ids(&[1, 7]), "foo%bar");
    assert_eq!(multiple, ids(&[2, 4, 5, 6]), "%a%c%");
    assert_eq!(single, ids(&[2, 4, 5]), "a_c");
    assert_eq!(repeated, ids(&[1, 7]), "foo%%bar");
    assert_eq!(escaped_percent, ids(&[4]), "escaped percent");
    assert_eq!(escaped_underscore, ids(&[5]), "escaped underscore");
    assert_eq!(case_sensitive, ids(&[3]), "LIKE is case-sensitive");
    assert_eq!(parameterized, ids(&[1, 7]), "parameterized pattern");
    assert_eq!(negated, ids(&[3]), "negated LIKE keeps NULL out");

    let _ = std::fs::remove_dir_all(path);
}
