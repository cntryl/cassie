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
fn should_execute_insert_values_with_explicit_columns_returning_columns() {
    // Arrange
    let path = data_dir("insert_values_returning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_returning (title TEXT, body TEXT)",
                vec![],
            )

.unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_returning (title, body) VALUES ('alpha', 'first') RETURNING title, body",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command(), "INSERT 0 1");
        assert_eq!(result.columns()[0].name, "title");
        assert_eq!(result.columns()[1].name, "body");
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows()[0][1], Value::String("first".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_stored_canonical_typed_text_from_insert_update_returning() {
    // Arrange
    let path = data_dir("insert_values_returning_canonical");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE returning_canonical (k INT, ts TIMESTAMP, tag UUID, blob BYTEA)",
                    vec![],
                )
                .unwrap();
            // PostgreSQL 18.6 returns timestamp text in ISO DateStyle
            // (docs/testing/oracle-corrections.md, ORACLE-T05).
            let expected = vec![
                Value::String("2024-06-01 08:00:00".to_string()),
                Value::String("6ba7b810-9dad-11d1-80b4-00c04fd430c8".to_string()),
                Value::String("\\xcafe".to_string()),
            ];

            // Act
            let autocommit = cassie
                .execute_sql(
                    &session,
                    "INSERT INTO returning_canonical (k, ts, tag, blob) VALUES (1, '2024-06-01 08:00:00', '6BA7B810-9DAD-11D1-80B4-00C04FD430C8', '\\xCAFE') RETURNING ts, tag, blob",
                    vec![],
                )
                .unwrap();
            cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
            let staged = cassie
                .execute_sql(
                    &session,
                    "INSERT INTO returning_canonical (k, ts, tag, blob) VALUES (2, $1, $2, $3) RETURNING ts, tag, blob",
                    vec![
                        Value::String("2024-06-01 08:00:00".to_string()),
                        Value::String("6BA7B810-9DAD-11D1-80B4-00C04FD430C8".to_string()),
                        Value::String("\\xCAFE".to_string()),
                    ],
                )
                .unwrap();
            let updated = cassie
                .execute_sql(
                    &session,
                    "UPDATE returning_canonical SET ts = '2024-06-01 08:00:00' WHERE k = 2 RETURNING ts, tag, blob",
                    vec![],
                )
                .unwrap();
            cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
            let stored = cassie
                .execute_sql(
                    &session,
                    "SELECT ts, tag, blob FROM returning_canonical ORDER BY k",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(autocommit.rows(), vec![expected.clone()]);
            assert_eq!(staged.rows(), vec![expected.clone()]);
            assert_eq!(updated.rows(), vec![expected.clone()]);
            assert_eq!(stored.rows(), vec![expected.clone(), expected]);

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_insert_values_using_table_column_order() {
    // Arrange
    let path = data_dir("insert_values_table_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_table_order (title TEXT, score INT)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_table_order VALUES ('alpha', 7)",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM insert_values_table_order",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows().len(), 1);
        assert_eq!(selected.rows()[0][0], Value::String("alpha".to_string()));
        assert_eq!(selected.rows()[0][1], Value::Int64(7));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_insert_multiple_values_rows() {
    // Arrange
    let path = data_dir("insert_multiple_values");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_multiple_values (title TEXT, score INT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_multiple_values (title, score) VALUES ('alpha', 1), ('beta', 2) RETURNING title, score",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.command(), "INSERT 0 2");
        assert_eq!(
            inserted.rows(),
            vec![
                vec![Value::String("alpha".to_string()), Value::Int64(1)],
                vec![Value::String("beta".to_string()), Value::Int64(2)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_generated_row_id_from_insert_values() {
    // Arrange
    let path = data_dir("insert_values_id");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_id (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_id (title) VALUES ('alpha') RETURNING _id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.columns()[0].name, "_id");
        assert_eq!(inserted.rows().len(), 1);
        assert!(matches!(&inserted.rows()[0][0], Value::String(id) if !id.is_empty()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_insert_returning_wildcard() {
    // Arrange
    let path = data_dir("insert_returning_wildcard");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_returning_wildcard (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_wildcard (title, body) VALUES ('alpha', 'first') RETURNING *",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.columns()[0].name, "_id");
        assert_eq!(inserted.columns()[1].name, "title");
        assert_eq!(inserted.columns()[2].name, "body");
        assert!(matches!(&inserted.rows()[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(inserted.rows()[0][1], Value::String("alpha".to_string()));
        assert_eq!(inserted.rows()[0][2], Value::String("first".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_insert_returning_scalar_function() {
    // Arrange
    let path = data_dir("insert_returning_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_returning_function (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_function (title) VALUES ('ALPHA') RETURNING lower(title) AS normalized",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.columns()[0].name, "normalized");
        assert_eq!(
            inserted.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_returning_unknown_function() {
    // Arrange
    let path = data_dir("insert_returning_unknown_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_returning_unknown_function (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_unknown_function (title) VALUES ('ALPHA') RETURNING missing_fn(title)",
                vec![],
            );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unsupported function"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_when_not_null_constraint_fails() {
    // Arrange
    let path = data_dir("insert_values_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_not_null (title TEXT NOT NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO insert_values_not_null (title) VALUES (NULL)",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("cannot be null"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_when_not_null_column_is_missing() {
    // Arrange
    let path = data_dir("insert_values_missing_not_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_missing_not_null (title TEXT NOT NULL, body TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO insert_values_missing_not_null (body) VALUES ('first')",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("cannot be null"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_default_values_for_insert_values() {
    // Arrange
    let path = data_dir("insert_values_defaults");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_defaults (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )

.unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_defaults (id) VALUES (1) RETURNING status",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.rows().len(), 1);
        assert_eq!(inserted.rows()[0][0], Value::String("pending".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_explicit_insert_value_when_default_exists() {
    // Arrange
    let path = data_dir("insert_values_explicit_default");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_explicit_default (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_explicit_default (id, status) VALUES (1, 'done') RETURNING status",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.rows().len(), 1);
        assert_eq!(inserted.rows()[0][0], Value::String("done".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_round_trip_insert_values_vector_field() {
    // Arrange
    let path = data_dir("insert_values_vector_round_trip");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_vector_round_trip (doc_id TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_vector_round_trip (doc_id, embedding) VALUES ('row-1', $1)",
                vec![Value::Vector(Vector::new(vec![1.0, 2.0, 3.0]))],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT embedding FROM insert_values_vector_round_trip WHERE doc_id = 'row-1'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::Vector(Vector::new(vec![1.0, 2.0, 3.0]))]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_when_vector_dimensions_mismatch() {
    // Arrange
    let path = data_dir("insert_values_vector_dimensions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_vector_dimensions (embedding VECTOR(2))",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO insert_values_vector_dimensions (embedding) VALUES ($1)",
            vec![Value::Vector(Vector::new(vec![1.0]))],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("expects vector(2)"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_with_duplicate_target_column() {
    // Arrange
    let path = data_dir("insert_duplicate_column");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_duplicate_column (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO insert_duplicate_column (title, title) VALUES ('alpha', 'beta')",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("duplicated"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_values_with_unknown_target_column() {
    // Arrange
    let path = data_dir("insert_unknown_column");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_unknown_column (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO insert_unknown_column (missing) VALUES ('alpha')",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("does not exist"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_store_insert_values_as_row_blobs() {
    // Arrange
    let path = data_dir("insert_values_row_blob");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_values_row_blob (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_row_blob (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let legacy_row_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"r/insert_values_row_blob/")
            .unwrap();
        let legacy_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:insert_values_row_blob:")
            .unwrap();
        let selected = cassie
            .execute_sql(&session, "SELECT title FROM insert_values_row_blob", vec![])
            .unwrap();

        // Assert
        assert_eq!(
            legacy_row_entries,
            [] as [(std::vec::Vec<u8>, std::vec::Vec<u8>); 0]
        );
        assert_eq!(
            legacy_entries,
            [] as [(std::vec::Vec<u8>, std::vec::Vec<u8>); 0]
        );
        assert_eq!(selected.rows()[0][0], Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_missing_sparse_row_fields_as_null() {
    // Arrange
    let path = data_dir("sparse_row_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sparse_row_projection (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sparse_row_projection (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM sparse_row_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows().len(), 1);
        assert_eq!(selected.rows()[0][0], Value::String("alpha".to_string()));
        assert_eq!(selected.rows()[0][1], Value::Null);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_an_invalid_timestamp_string_on_insert() {
    // Arrange
    let path = data_dir("insert_invalid_timestamp");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_invalid_timestamp (id INT, ts TIMESTAMP)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO insert_invalid_timestamp (id, ts) VALUES (1, 'not-a-timestamp')",
            vec![],
        );

        // Assert
        assert!(
            inserted.is_err(),
            "an unparseable TIMESTAMP string must be rejected on write"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cast_a_timestamp_string_to_its_canonical_utc_form() {
    // Arrange
    let path = data_dir("cast_timestamp_canonical");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let cast_offset = cassie
            .execute_sql(
                &session,
                "SELECT CAST('2024-01-01T09:00:00+02:00' AS TIMESTAMP) AS ts",
                vec![],
            )
            .unwrap();
        let cast_invalid = cassie.execute_sql(
            &session,
            "SELECT CAST('not-a-timestamp' AS TIMESTAMP) AS ts",
            vec![],
        );

        // Assert: PostgreSQL 18.6 ignores the offset of `timestamp` input
        // (see docs/testing/oracle-corrections.md, ORACLE-T01).
        assert_eq!(
            cast_offset.rows()[0][0],
            Value::String("2024-01-01 09:00:00".to_string())
        );
        assert!(
            cast_invalid.is_err(),
            "casting an invalid string to TIMESTAMP must fail"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
