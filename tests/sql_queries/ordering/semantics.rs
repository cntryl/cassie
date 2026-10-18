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
fn should_keep_order_by_direction_through_distinct() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "distinct_keeps_order",
        &[
            "CREATE TABLE np (category TEXT)",
            "INSERT INTO np (category) VALUES ('b')",
            "INSERT INTO np (category) VALUES (NULL)",
            "INSERT INTO np (category) VALUES ('a')",
            "INSERT INTO np (category) VALUES ('a')",
            "CREATE TABLE d (n INT)",
            "INSERT INTO d (n) VALUES (1)",
            "INSERT INTO d (n) VALUES (3)",
            "INSERT INTO d (n) VALUES (2)",
            "INSERT INTO d (n) VALUES (3)",
            "INSERT INTO d (n) VALUES (NULL)",
        ],
    );
    let text = |value: &str| Value::String(value.to_string());

    // Act
    let ascending = fixture.rows("SELECT DISTINCT category FROM np ORDER BY category");
    let descending = fixture.rows("SELECT DISTINCT category FROM np ORDER BY category DESC");
    let numeric = fixture.rows("SELECT DISTINCT n FROM d ORDER BY n DESC");
    let limited = fixture.rows("SELECT DISTINCT n FROM d ORDER BY n DESC LIMIT 2");

    // Assert
    assert_eq!(
        ascending,
        vec![vec![text("a")], vec![text("b")], vec![Value::Null]]
    );
    assert_eq!(
        descending,
        vec![vec![Value::Null], vec![text("b")], vec![text("a")]]
    );
    assert_eq!(
        numeric,
        vec![
            vec![Value::Null],
            vec![Value::Int64(3)],
            vec![Value::Int64(2)],
            vec![Value::Int64(1)]
        ]
    );
    assert_eq!(limited, vec![vec![Value::Null], vec![Value::Int64(3)]]);
}

#[test]
fn should_apply_limit_offset_after_ordering() {
    // Arrange
    use_local_storage();
    let path = data_dir("limit_offset_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let collection = "sql_limit_offset_order";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "pear", "body": "c"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "apple", "body": "a"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "banana", "body": "b"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_limit_offset_order ORDER BY title ASC LIMIT 2 OFFSET 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.columns().len(), 2);
        assert_eq!(result.columns()[0].name, "id");
        assert_eq!(result.columns()[1].name, "title");
        assert_eq!(result.rows().len(), 2);

        let rows = result.rows();
        let ids = rows
            .iter()
            .map(|row| match &row[0] {
                cassie::types::Value::String(id) => id.clone(),
                _ => panic!("expected string id"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["d3".to_string(), "d1".to_string()]);

        let titles = rows
            .iter()
            .map(|row| match &row[1] {
                cassie::types::Value::String(title) => title.clone(),
                _ => panic!("expected string title"),
            })
            .collect::<Vec<_>>();
        assert_eq!(titles, vec!["banana".to_string(), "pear".to_string()]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_page_by_row_id_with_storage_top_k() {
    // Arrange
    use_local_storage();
    let path = data_dir("row_id_storage_top_k");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_row_id_storage_top_k";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        for (id, title) in [("d3", "three"), ("d1", "one"), ("d2", "two")] {
            cassie
                .midge
                .put_document(
                    collection,
                    Some(id.to_string()),
                    serde_json::json!({"title": title}),
                )
                .unwrap();
        }

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_row_id_storage_top_k ORDER BY id ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows().len(), 2);
        assert_eq!(result.rows()[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows()[0][1], Value::String("one".to_string()));
        assert_eq!(result.rows()[1][0], Value::String("d2".to_string()));
        assert_eq!(result.rows()[1][1], Value::String("two".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_page_by_row_id_with_keyset_cursor() {
    // Arrange
    use_local_storage();
    let path = data_dir("row_id_keyset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_row_id_keyset";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        for (id, title) in [("d1", "one"), ("d2", "two"), ("d3", "three")] {
            cassie
                .midge
                .put_document(
                    collection,
                    Some(id.to_string()),
                    serde_json::json!({"title": title}),
                )
                .unwrap();
        }

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_row_id_keyset WHERE id > 'd1' ORDER BY id ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows().len(), 2);
        assert_eq!(result.rows()[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows()[0][1], Value::String("two".to_string()));
        assert_eq!(result.rows()[1][0], Value::String("d3".to_string()));
        assert_eq!(result.rows()[1][1], Value::String("three".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_projected_scan_range_query_without_changing_results() {
    // Arrange
    use_local_storage();
    let path = data_dir("projected_scan_range");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_range";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "low", "score": 1}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "mid", "score": 10}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "high", "score": 20}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_projected_scan_range WHERE score >= 10 LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows().len(), 2);
        assert_eq!(result.rows()[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows()[0][1], Value::String("mid".to_string()));
        assert_eq!(result.rows()[1][0], Value::String("d3".to_string()));
        assert_eq!(result.rows()[1][1], Value::String("high".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_projected_scan_simple_equality_query_without_changing_results() {
    // Arrange
    use_local_storage();
    let path = data_dir("projected_scan_equality");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_equality";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "first"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "second"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_projected_scan_equality WHERE title = 'beta'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows()[0][1], Value::String("beta".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_vector_distance_offset_after_ordering() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_distance_offset_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_vector_distance_offset_order";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(3),
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [3.0, 0.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM sql_vector_distance_offset_order ORDER BY distance ASC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], Value::String("d3".to_string()));
        assert_eq!(result.rows()[0][1], Value::Float64(1.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_fulltext_offset_after_score_ordering() {
    // Arrange
    use_local_storage();
    let path = data_dir("fulltext_top_k_offset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_top_k_offset";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "alpha alpha"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_top_k_offset WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_hybrid_offset_after_score_ordering() {
    // Arrange
    use_local_storage();
    let path = data_dir("hybrid_top_k_offset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_top_k_offset";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "red red", "embedding": [2.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_top_k_offset ORDER BY score DESC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_timestamps_with_mixed_utc_offsets_by_instant() {
    // Arrange
    use_local_storage();
    let path = data_dir("timestamp_mixed_offset_order");
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
                    "CREATE TABLE timestamp_mixed_offset_order (seq INT, ts TIMESTAMP)",
                    vec![],
                )
                .unwrap();
            // Row 1 is 2024-01-01T07:00:00Z once its +02:00 offset is applied,
            // an hour before row 2's plain UTC instant.
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO timestamp_mixed_offset_order (seq, ts) VALUES (1, '2024-01-01T09:00:00+02:00')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO timestamp_mixed_offset_order (seq, ts) VALUES (2, '2024-01-01T08:00:00Z')",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT seq FROM timestamp_mixed_offset_order ORDER BY ts ASC",
                    vec![],
                )
                .unwrap();

            // Assert: row 1's instant (07:00Z) is earlier than row 2's (08:00Z).
            assert_eq!(
                result.rows(),
                vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

fn ordered_case_session(
    label: &str,
    statements: &[&str],
) -> (Cassie, cassie::app::CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in statements {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }
    (cassie, session, path)
}

#[test]
fn should_top_k_order_a_mixed_case_column_referenced_in_lowercase() {
    // Arrange
    let (cassie, session, path) = ordered_case_session(
        "ordered_top_k_declared_case",
        &[
            "CREATE TABLE mc (Label TEXT, Score INT)",
            "INSERT INTO mc (Label, Score) VALUES ('low', 1), ('high', 9), ('mid', 5)",
        ],
    );

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT Label, Score FROM mc ORDER BY score DESC LIMIT 2",
            vec![],
        )
        .expect("ordered top-k");

    // Assert
    assert_eq!(
        result.rows(),
        vec![
            vec![Value::String("high".to_string()), Value::Int64(9)],
            vec![Value::String("mid".to_string()), Value::Int64(5)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_top_k_order_a_lowercase_column_referenced_in_uppercase() {
    // Arrange
    let (cassie, session, path) = ordered_case_session(
            "ordered_top_k_query_case",
            &[
                "CREATE TABLE oc (v INT, w TEXT)",
                "INSERT INTO oc (v, w) VALUES (5, 'e'), (1, 'a'), (4, 'd'), (2, 'b'), (3, 'c'), (7, 'g'), (6, 'f')",
            ],
        );

    // Act
    let ascending = cassie
        .execute_sql(&session, "SELECT w FROM oc ORDER BY V LIMIT 4", vec![])
        .expect("ascending top-k");
    let descending = cassie
        .execute_sql(&session, "SELECT w FROM oc ORDER BY V DESC LIMIT 2", vec![])
        .expect("descending top-k");

    // Assert
    let text = |value: &str| vec![Value::String(value.to_string())];
    assert_eq!(
        ascending.rows(),
        vec![text("a"), text("b"), text("c"), text("d")]
    );
    assert_eq!(descending.rows(), vec![text("g"), text("f")]);
    let _ = std::fs::remove_dir_all(path);
}
