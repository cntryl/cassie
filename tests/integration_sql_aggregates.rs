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

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_execute_grouped_count_query() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_count");
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
                "CREATE TABLE aggregate_count_docs (category TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('b')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM aggregate_count_docs GROUP BY category ORDER BY category",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("a".to_string()), Value::Int64(2)],
                vec![Value::String("b".to_string()), Value::Int64(1)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_basic_numeric_aggregates_query() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_numeric");
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
                "CREATE TABLE aggregate_numeric_sales (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (5)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_numeric_sales",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::Int64(15),
                Value::Float64(5.0),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_null_values_for_basic_aggregates_query() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_nulls");
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
                "CREATE TABLE aggregate_null_sales (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_null_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (NULL)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_null_sales",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::Int64(2),
                Value::Int64(10),
                Value::Float64(5.0),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_row_number_window_function_query() {
    // Arrange
    with_fallback();
    let path = data_dir("window_row_number");
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
                "CREATE TABLE window_scores (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'first', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'second', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('b', 'third', 30)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, title, row_number() OVER (PARTITION BY category ORDER BY score DESC) AS rank FROM window_scores ORDER BY category ASC, rank ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("a".to_string()),
                    Value::String("second".to_string()),
                    Value::Int64(1)
                ],
                vec![
                    Value::String("a".to_string()),
                    Value::String("first".to_string()),
                    Value::Int64(2)
                ],
                vec![
                    Value::String("b".to_string()),
                    Value::String("third".to_string()),
                    Value::Int64(1)
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_basic_value_window_functions_query() {
    // Arrange
    with_fallback();
    let path = data_dir("window_basic_values");
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
                "CREATE TABLE window_values (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'alpha', 30)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'beta', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'gamma', 20)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS rnk, dense_rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS dense, lag(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS prev, lead(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS next, first_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS first, last_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS last FROM window_values ORDER BY rnk ASC, title ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("alpha".to_string()),
                    Value::Int64(1),
                    Value::Int64(1),
                    Value::Null,
                    Value::String("beta".to_string()),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
                ],
                vec![
                    Value::String("beta".to_string()),
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string()),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
                ],
                vec![
                    Value::String("gamma".to_string()),
                    Value::Int64(3),
                    Value::Int64(3),
                    Value::String("beta".to_string()),
                    Value::Null,
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_grouped_rows_with_having() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_having");
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
                "CREATE TABLE aggregate_having_sales (category TEXT, amount INT)",
                vec![],
            )

.unwrap();
        for sql in [
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 7)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 5)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('b', 3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, SUM(amount) AS total FROM aggregate_having_sales GROUP BY category HAVING SUM(amount) > 10",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("a".to_string()), Value::Int64(12)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
