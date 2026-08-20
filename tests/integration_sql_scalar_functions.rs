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

// should_execute_text_scalar_functions_query removed: strictly subsumed by
// scalar_functions.rs::should_execute_string_scalar_functions_in_query_path,
// which covers the same lower/upper/trim/substring/concat/length assertions
// plus len()/WHERE/ORDER BY on the same operations.

#[test]
fn should_execute_coalesce_scalar_function_query() {
    // Arrange
    use_local_storage();
    let path = data_dir("scalar_coalesce_function");
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
                "CREATE TABLE scalar_coalesce_function (title TEXT, fallback TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_coalesce_function (title, fallback) VALUES (NULL, 'backup')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT coalesce(title, fallback, 'missing') AS value FROM scalar_coalesce_function",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("backup".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_numeric_scalar_function_query() {
    // Arrange
    use_local_storage();
    let path = data_dir("scalar_numeric_function");
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
                "CREATE TABLE scalar_numeric_function (delta INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_numeric_function (delta) VALUES (-42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT abs(delta) AS magnitude FROM scalar_numeric_function",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::Int64(42)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_abs_overflow_for_bigint_minimum() {
    // Arrange
    use_local_storage();
    let path = data_dir("abs_bigint_overflow");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let overflow = cassie.execute_sql(
            &session,
            "SELECT ABS(CAST('-9223372036854775808' AS BIGINT)) AS result",
            vec![],
        );

        // Assert
        assert!(overflow
            .expect_err("reject an unrepresentable BIGINT magnitude")
            .to_string()
            .contains("integer overflow"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_abs_at_numeric_boundaries() {
    // Arrange
    use_local_storage();
    let path = data_dir("abs_numeric_boundaries");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let adjacent = cassie
            .execute_sql(
                &session,
                "SELECT ABS(CAST('-9223372036854775807' AS BIGINT)), ABS(CAST('9223372036854775807' AS BIGINT)), ABS(CAST('-1.5' AS FLOAT))",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            adjacent.rows,
            vec![vec![
                Value::Int64(i64::MAX),
                Value::Int64(i64::MAX),
                Value::Float64(1.5),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_cast_function_expression() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_cast_function");
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
                "CREATE TABLE predicate_cast_function (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_cast_function (title, score) VALUES ('alpha', 10)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_cast_function WHERE CAST(score AS TEXT) = '10'",
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
fn should_filter_rows_with_postgres_style_cast_expression() {
    // Arrange
    use_local_storage();
    let path = data_dir("predicate_pg_cast");
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
                "CREATE TABLE predicate_pg_cast (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_pg_cast (title, score) VALUES ('alpha', 10)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_pg_cast WHERE score::TEXT = '10'",
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
fn should_project_rows_with_cast_expressions() {
    // Arrange
    use_local_storage();
    let path = data_dir("projection_cast_expressions");
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
                "CREATE TABLE projection_cast_expressions (score INT, active BOOLEAN, flag TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_cast_expressions (score, active, flag) VALUES (10, true, 't')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT CAST(score AS TEXT) AS score_text, score::FLOAT AS score_float, CAST(active AS INT) AS active_int, CAST(flag AS BOOLEAN) AS flag_bool FROM projection_cast_expressions",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("10".to_string()),
                Value::Float64(10.0),
                Value::Int64(1),
                Value::Bool(true)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_invalid_cast_expression() {
    // Arrange
    use_local_storage();
    let path = data_dir("invalid_cast_expression");
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
                "CREATE TABLE invalid_cast_expression (label TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO invalid_cast_expression (label) VALUES ('not-a-number')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie.execute_sql(
            &session,
            "SELECT CAST(label AS INT) FROM invalid_cast_expression",
            vec![],
        );

        // Assert
        assert!(selected.is_err());
        assert!(selected
            .unwrap_err()
            .to_string()
            .contains("cannot cast value to INT"));

        let _ = std::fs::remove_dir_all(path);
    });
}
