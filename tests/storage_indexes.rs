// Consolidated integration suite: storage_indexes.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/local_storage.rs"]
mod support_local_storage;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/index_publication_recovery.rs.
mod index_publication_recovery {
    use std::collections::BTreeMap;

    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::midge::adapter::set_index_publication_failure_point;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{
        canonical_test_collection, data_dir, openai_runtime_for_vectors, use_local_storage,
    };

    static INDEX_PUBLICATION_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn should_replay_prepared_scalar_index_publication_after_restart() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = INDEX_PUBLICATION_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("index_publication_recovery");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE index_publication_docs (title TEXT)",
                    vec![],
                )
                .expect("create table");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO index_publication_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .expect("seed row");
            let collection = canonical_test_collection(&cassie, "index_publication_docs");
            let index = IndexMeta {
                collection: collection.clone(),
                name: "index_publication_docs_title_idx".to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: vec![],
                include_fields: vec![],
                predicate: None,
                kind: IndexKind::Scalar,
                unique: false,
                options: BTreeMap::new(),
            };

            // Act
            set_index_publication_failure_point(true);
            assert!(cassie.midge.put_index(&index).is_err());
            assert!(cassie
                .midge
                .get_index(&collection, &index.name)
                .expect("read unpublished index")
                .is_none());
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
            restarted.startup().expect("replay prepared index");
            let restarted_session = restarted.create_session("tester", None);
            let result = restarted
                .execute_sql(
                    &restarted_session,
                    "SELECT title FROM index_publication_docs WHERE title = 'alpha'",
                    vec![],
                )
                .expect("query after replay");

            // Assert
            assert!(restarted
                .midge
                .get_index(&collection, &index.name)
                .expect("read published index")
                .is_some());
            assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_prepared_fulltext_index_publication_after_restart() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = INDEX_PUBLICATION_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("fulltext_index_publication_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fulltext_index_publication_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fulltext_index_publication_docs (title) VALUES ('alpha beta')",
                vec![],
            )
            .expect("seed row");
        let collection = canonical_test_collection(&cassie, "fulltext_index_publication_docs");
        let index = IndexMeta {
            collection: collection.clone(),
            name: "fulltext_index_publication_docs_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: vec![],
            include_fields: vec![],
            predicate: None,
            kind: IndexKind::FullText,
            unique: false,
            options: BTreeMap::new(),
        };

        // Act
        set_index_publication_failure_point(true);
        assert!(cassie.midge.put_index(&index).is_err());
        assert!(cassie
            .midge
            .get_index(&collection, &index.name)
            .expect("read unpublished index")
            .is_none());
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay prepared index");
        let restarted_session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT title FROM fulltext_index_publication_docs WHERE search(title, 'alpha')",
                vec![],
            )
            .expect("query after replay");

        // Assert
        assert!(restarted
            .midge
            .get_index(&collection, &index.name)
            .expect("read published index")
            .is_some());
        assert_eq!(
            result.rows,
            vec![vec![Value::String("alpha beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_hide_vector_index_until_prepared_publication_replays() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = INDEX_PUBLICATION_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("vector_index_publication_recovery");
        let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors())
            .expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_index_publication_docs (content TEXT, embedding VECTOR(1536))",
                vec![],
            )
            .expect("create table");

        // Act
        set_index_publication_failure_point(true);
        assert!(cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_index_publication_idx ON vector_index_publication_docs USING vector (embedding) WITH (source_field = content, metric = l2)",
            vec![],
        )
        .is_err());

        // Assert
        assert!(cassie
            .catalog
            .list_vector_indexes("vector_index_publication_docs")
            .is_empty());
        assert!(cassie
            .catalog
            .get_index(
                "vector_index_publication_docs",
                "vector_index_publication_idx"
            )
            .is_none());
        assert!(cassie
            .midge
            .get_vector_index("vector_index_publication_docs", "embedding")
            .expect("read prepared vector metadata")
            .is_some());

        drop(cassie);
        let restarted = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors())
            .expect("reopen Cassie");
        restarted.startup().expect("replay prepared vector index");
        assert_eq!(
            restarted
                .catalog
                .list_vector_indexes("vector_index_publication_docs")
                .len(),
            1
        );
        assert!(restarted
            .catalog
            .get_index(
                "vector_index_publication_docs",
                "vector_index_publication_idx"
            )
            .is_some());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_column_batches_before_prepared_publication_replays() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = INDEX_PUBLICATION_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("column_index_publication_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_index_publication_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_index_publication_docs (title) VALUES ('alpha')",
                vec![],
            )
            .expect("seed row");

        // Act
        set_index_publication_failure_point(true);
        assert!(cassie
        .execute_sql(
            &session,
            "CREATE INDEX column_index_publication_idx ON column_index_publication_docs USING column (title)",
            vec![],
        )
        .is_err());
        let collection = canonical_test_collection(&cassie, "column_index_publication_docs");
        assert!(cassie
            .midge
            .get_index(&collection, "column_index_publication_idx")
            .expect("read unpublished column index")
            .is_none());
        assert!(cassie
            .midge
            .get_column_batch_metadata(&collection, "column_index_publication_idx")
            .expect("read unpublished column batches")
            .is_none());
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay prepared column index");

        // Assert
        let index = restarted
            .midge
            .get_index(&collection, "column_index_publication_idx")
            .expect("read replayed column index")
            .expect("column index after replay");
        let metadata = restarted
            .midge
            .get_column_batch_metadata(&collection, &index.name)
            .expect("read replayed column batches")
            .expect("column batches after replay");
        assert_eq!(
            metadata.built_generation,
            restarted
                .midge
                .collection_generation(&collection)
                .expect("collection generation")
        );
        assert_eq!(metadata.segments.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/integration_sql_scalar_index_lexkey.rs.
mod integration_sql_scalar_index_lexkey {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::*;

    #[test]
    fn should_scan_scalar_index_with_signed_float_bounds() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_lexkey_numeric_bounds");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE scalar_lexkey_numeric_bounds (score INT, rating FLOAT)",
                    vec![],
                )
                .unwrap();
            for (id, score, rating) in [
                ("row-1", -10, -2.5),
                ("row-2", -2, -1.25),
                ("row-3", 0, 0.5),
                ("row-4", 7, 3.75),
            ] {
                cassie
                    .midge
                    .put_document(
                        "scalar_lexkey_numeric_bounds",
                        Some(id.to_string()),
                        serde_json::json!({"score": score, "rating": rating}),
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX scalar_lexkey_score_idx ON scalar_lexkey_numeric_bounds USING btree (score)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX scalar_lexkey_rating_idx ON scalar_lexkey_numeric_bounds USING btree (rating)",
                    vec![],
                )
                .unwrap();

            // Act
            let scores = cassie
                .execute_sql(
                    &session,
                    "SELECT score FROM scalar_lexkey_numeric_bounds WHERE score >= -2 AND score < 7 ORDER BY score",
                    vec![],
                )
                .unwrap();
            let ratings = cassie
                .execute_sql(
                    &session,
                    "SELECT rating FROM scalar_lexkey_numeric_bounds WHERE rating > -2.5 AND rating <= 0.5 ORDER BY rating",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                scores.rows,
                vec![vec![Value::Int64(-2)], vec![Value::Int64(0)]]
            );
            assert_eq!(
                ratings.rows,
                vec![vec![Value::Float64(-1.25)], vec![Value::Float64(0.5)]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_match_integer_shaped_literals_in_float_scalar_indexes() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_lexkey_whole_float");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            for table in ["whole_float_baseline", "whole_float_indexed"] {
                cassie
                    .execute_sql(
                        &session,
                        &format!("CREATE TABLE {table} (row_number INT, rating FLOAT)"),
                        vec![],
                    )
                    .expect("create float table");
                cassie
                    .execute_sql(
                        &session,
                        &format!("INSERT INTO {table} (row_number, rating) VALUES (1, 100.0)"),
                        vec![],
                    )
                    .expect("insert whole-number float");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX whole_float_rating_idx ON whole_float_indexed USING btree (rating)",
                    vec![],
                )
                .expect("create float scalar index");

            // Act
            let baseline_integer = cassie
                .execute_sql(
                    &session,
                    "SELECT row_number FROM whole_float_baseline WHERE rating = 100",
                    vec![],
                )
                .expect("query unindexed float with integer literal");
            let indexed_integer = cassie
                .execute_sql(
                    &session,
                    "SELECT row_number FROM whole_float_indexed WHERE rating = 100",
                    vec![],
                )
                .expect("query indexed float with integer literal");
            let baseline_float = cassie
                .execute_sql(
                    &session,
                    "SELECT row_number FROM whole_float_baseline WHERE rating = 100.0",
                    vec![],
                )
                .expect("query unindexed float with float literal");
            let indexed_float = cassie
                .execute_sql(
                    &session,
                    "SELECT row_number FROM whole_float_indexed WHERE rating = 100.0",
                    vec![],
                )
                .expect("query indexed float with float literal");
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT row_number FROM whole_float_indexed WHERE rating = 100",
                    vec![],
                )
                .expect("explain float scalar index lookup");

            // Assert
            let expected = vec![vec![Value::Int64(1)]];
            assert_eq!(baseline_integer.rows, expected);
            assert_eq!(indexed_integer.rows, baseline_integer.rows);
            assert_eq!(baseline_float.rows, expected);
            assert_eq!(indexed_float.rows, baseline_float.rows);
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=whole_float_rating_idx"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_scan_composite_scalar_index_with_embedded_nul_text() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_lexkey_nul_text");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE scalar_lexkey_nul_text (tenant TEXT, label TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "scalar_lexkey_nul_text",
                    Some("row-1".to_string()),
                    serde_json::json!({"tenant": "acme", "label": "aa"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "scalar_lexkey_nul_text",
                    Some("row-2".to_string()),
                    serde_json::json!({"tenant": "acme", "label": "a\u{0}a"}),
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX scalar_lexkey_tenant_label_idx ON scalar_lexkey_nul_text USING btree (tenant, label)",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM scalar_lexkey_nul_text WHERE tenant = 'acme' AND label >= 'a' ORDER BY label",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT label FROM scalar_lexkey_nul_text WHERE tenant = 'acme' AND label >= 'a' ORDER BY label",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("a\u{0}a".to_string())],
                    vec![Value::String("aa".to_string())],
                ]
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("index=scalar_lexkey_tenant_label_idx"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/integration_sql_scalar_indexes.rs.
mod integration_sql_scalar_indexes {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_explain_index_aware_plan_for_scalar_equality_filter() {
        // Arrange
        use_local_storage();
        let path = data_dir("explain_index_aware");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_explain_index_aware (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_index_aware_email_idx ON sql_explain_index_aware USING btree (email)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_index_aware WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index_aware=true"));
        assert!(plan.contains("index=sql_explain_index_aware_email_idx"));
        assert!(plan.contains("access_path=index_seek"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_use_covering_scalar_index_for_projection() {
        // Arrange
        use_local_storage();
        let path = data_dir("covering_scalar_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_covering_scalar_projection (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_covering_scalar_projection (email, title) VALUES ('a@example.com', 'alpha'), ('b@example.com', 'beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_covering_scalar_projection_email_idx ON sql_covering_scalar_projection USING btree (email)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT email FROM sql_covering_scalar_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT email FROM sql_covering_scalar_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("a@example.com".to_string())]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=sql_covering_scalar_projection_email_idx"));
        assert!(plan.contains("covered_index=true"));
        assert_eq!(
            after["covering_indexes"]["scans"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["scans"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["covering_indexes"]["row_fetches_avoided"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["row_fetches_avoided"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fallback_for_noncovered_scalar_index_projection() {
        // Arrange
        use_local_storage();
        let path = data_dir("covering_scalar_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_covering_scalar_fallback (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_covering_scalar_fallback (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_covering_scalar_fallback_email_idx ON sql_covering_scalar_fallback USING btree (email)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_covering_scalar_fallback WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_covering_scalar_fallback WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=sql_covering_scalar_fallback_email_idx"));
        assert!(plan.contains("covered_index=false"));
        assert_eq!(
            after["covering_indexes"]["fallback_scans"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["fallback_scans"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_preserve_covering_scalar_index_order_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("covering_scalar_restart_order");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_covering_scalar_restart_order (email TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            let collection = cassie
                .catalog
                .get_schema("sql_covering_scalar_restart_order")
                .expect("catalog collection")
                .collection;
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-2".to_string()),
                    serde_json::json!({"email": "a@example.com", "title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"email": "a@example.com"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-3".to_string()),
                    serde_json::json!({"email": "b@example.com", "title": "beta"}),
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_covering_scalar_restart_order_email_idx ON sql_covering_scalar_restart_order USING btree (email)",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT id, email FROM sql_covering_scalar_restart_order WHERE email = 'a@example.com' ORDER BY id DESC",
                vec![],
            )
            .unwrap();
        let explain = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, email FROM sql_covering_scalar_restart_order WHERE email = 'a@example.com' ORDER BY id DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("doc-2".to_string()),
                    Value::String("a@example.com".to_string())
                ],
                vec![
                    Value::String("doc-1".to_string()),
                    Value::String("a@example.com".to_string())
                ],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("covered_index=true"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_persist_include_index_metadata_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("include_metadata_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_include_metadata_restart (email TEXT, title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_include_metadata_restart_email_idx ON sql_include_metadata_restart USING btree (email) INCLUDE (title, body)",
                    vec![],
                )
                .unwrap();
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let index = restarted
            .catalog
            .get_index(
                "sql_include_metadata_restart",
                "sql_include_metadata_restart_email_idx",
            )
            .expect("index should hydrate");
        let session = restarted.create_session("tester", None);
        let introspection = restarted
            .execute_sql(
                &session,
                "SELECT indexdef FROM pg_catalog.pg_indexes WHERE tablename = 'sql_include_metadata_restart'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            index.include_fields,
            vec!["title".to_string(), "body".to_string()]
        );
        let Value::String(indexdef) = &introspection.rows[0][0] else {
            panic!("expected textual index definition");
        };
        assert!(indexdef.contains("INCLUDE (title, body)"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_use_include_columns_for_covered_projection() {
        // Arrange
        use_local_storage();
        let path = data_dir("include_covered_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_include_covered_projection (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_include_covered_projection (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_include_covered_projection_email_idx ON sql_include_covered_projection USING btree (email) INCLUDE (title)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_include_covered_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_include_covered_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("covered_index=true"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_hydrate_partial_index_metadata_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("partial_index_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_partial_index_restart (title TEXT, status TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_partial_index_restart_title_idx ON sql_partial_index_restart USING btree (title) WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let index = restarted
            .catalog
            .get_index(
                "sql_partial_index_restart",
                "sql_partial_index_restart_title_idx",
            )
            .expect("partial index should hydrate");
        let selected = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_partial_index_restart WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let fallback = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_partial_index_restart WHERE title = 'beta'",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(index.predicate.is_some());
        let Value::String(selected_plan) = &selected.rows[0][0] else {
            panic!("expected selected plan text");
        };
        assert!(selected_plan.contains("sql_partial_index_restart_title_idx"));
        let Value::String(fallback_plan) = &fallback.rows[0][0] else {
            panic!("expected fallback plan text");
        };
        assert!(fallback_plan.contains("index=none"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_query_rows_after_creating_secondary_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("secondary_index_query");
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
                "CREATE TABLE secondary_index_query (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX secondary_email_idx ON secondary_index_query USING btree (email)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('b@example.com', 'beta')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM secondary_index_query WHERE email = 'b@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_query_rows_after_creating_composite_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("composite_index_query");
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
                "CREATE TABLE composite_index_query (tenant_id TEXT, status TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX composite_tenant_status_idx ON composite_index_query USING btree (tenant_id, status)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'open', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'closed', 'beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-b', 'closed', 'gamma')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM composite_index_query WHERE tenant_id = 'tenant-a' AND status = 'closed'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM composite_index_query WHERE tenant_id = 'tenant-a' AND status = 'closed'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("beta".to_string())]]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=prefix_scan"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_query_rows_after_creating_range_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("range_index_query");
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
                "CREATE TABLE range_index_query (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX range_title_idx ON range_index_query USING btree (title)",
                vec![],
            )
            .unwrap();
        for (title, body) in [
            ("alpha", "a"),
            ("beta", "b"),
            ("delta", "d"),
            ("omega", "o"),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO range_index_query (title, body) VALUES ($1, $2)",
                    vec![Value::String(title.to_string()), Value::String(body.to_string())],
                )
                .unwrap();
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM range_index_query WHERE title >= 'beta' AND title < 'omega' ORDER BY title ASC",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM range_index_query WHERE title >= 'beta' AND title < 'omega' ORDER BY title ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("beta".to_string())],
                vec![Value::String("delta".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=range_scan"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_page_tenant_filtered_rows_through_composite_range_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("tenant_filtered_page_index_query");
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
                "CREATE TABLE tenant_filtered_page_index_query (tenant_id TEXT, status TEXT, created_at INT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX tenant_filtered_page_lookup_idx ON tenant_filtered_page_index_query USING btree (tenant_id, status, created_at)",
                vec![],
            )
            .unwrap();
        for (tenant_id, status, created_at, title) in [
            ("tenant-a", "open", 10, "alpha"),
            ("tenant-a", "open", 20, "beta"),
            ("tenant-a", "open", 30, "gamma"),
            ("tenant-a", "closed", 40, "closed"),
            ("tenant-b", "open", 50, "other"),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO tenant_filtered_page_index_query (tenant_id, status, created_at, title) VALUES ($1, $2, $3, $4)",
                    vec![
                        Value::String(tenant_id.to_string()),
                        Value::String(status.to_string()),
                        Value::Int64(created_at),
                        Value::String(title.to_string()),
                    ],
                )
                .unwrap();
        }
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM tenant_filtered_page_index_query WHERE tenant_id = 'tenant-a' AND status = 'open' AND created_at >= 10 ORDER BY created_at DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM tenant_filtered_page_index_query WHERE tenant_id = 'tenant-a' AND status = 'open' AND created_at >= 10 ORDER BY created_at DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("gamma".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("tenant_filtered_page_lookup_idx"), "plan={plan}");
        assert!(plan.contains("access_path=range_scan"));
        assert!(plan.contains("access_path_reason=scalar-index-range"));
        assert!(plan.contains("pagination_strategy=limit"));
        assert!(plan.contains("early_stop=scan_limit"));
        assert!(plan.contains("fallback_reason=none"));
        assert_eq!(
            after["read_paths"]["range_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["range_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert!(
            after["read_paths"]["last_index_scan_index"]
                .as_str()
                .is_some_and(|value| value.ends_with("tenant_filtered_page_lookup_idx"))
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_query_rows_after_creating_ordered_bounded_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("ordered_bounded_index_query");
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
                "CREATE TABLE ordered_bounded_index_query (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX ordered_bounded_title_idx ON ordered_bounded_index_query USING btree (title)",
                vec![],
            )
            .unwrap();
        for (title, body) in [("delta", "d"), ("alpha", "a"), ("charlie", "c"), ("beta", "b")] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO ordered_bounded_index_query (title, body) VALUES ($1, $2)",
                    vec![Value::String(title.to_string()), Value::String(body.to_string())],
                )
                .unwrap();
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM ordered_bounded_index_query ORDER BY title ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM ordered_bounded_index_query ORDER BY title ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=ordered_bounded_scan"));
        assert!(plan.contains("top_k_mode=storage"));

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/key_encoding_segment_compaction.rs.
mod key_encoding_segment_compaction {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::app::CassieSession;
    use cassie::midge::adapter::StorageFamily;
    use cntryl_lexkey::LexKey;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_pack_internal_key_segments_with_compact_markers() {
        // Arrange
        use_local_storage();
        let path = data_dir("key_segment_compaction");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            // Act
            populate_compaction_fixtures(&cassie, &session);
            // Assert
            assert_key_encoding_marker_compaction(
                &cassie
                    .midge
                    .raw_scan_prefix(StorageFamily::Data, b"")
                    .unwrap(),
            );
            let _ = std::fs::remove_dir_all(path);
        });
    }

    fn populate_compaction_fixtures(cassie: &Cassie, session: &CassieSession) {
        execute_all_queries(cassie, session, SCALAR_INDEX_QUERIES);
        execute_all_queries(cassie, session, GRAPH_QUERIES);
        execute_all_queries(cassie, session, FULLTEXT_INDEX_QUERIES);
    }

    fn execute_all_queries(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
        for statement in statements {
            let _ = cassie.execute_sql(session, statement, vec![]).unwrap();
        }
    }

    fn assert_key_encoding_marker_compaction(data_keys: &[(Vec<u8>, Vec<u8>)]) {
        assert!(data_keys
            .iter()
            .any(|(key, _)| key_family(key) == Some(b"\x11".as_slice())));
        assert!(data_keys
            .iter()
            .any(|(key, _)| key_family(key) == Some(b"\x15".as_slice())));
        assert!(data_keys
            .iter()
            .any(|(key, _)| key_family(key) == Some(b"\x17".as_slice())));
        assert!(data_keys
            .iter()
            .any(|(key, _)| key_family(key) == Some(b"\x18".as_slice())));
        assert!(data_keys
            .iter()
            .any(|(key, _)| key_family(key) == Some(b"\x16".as_slice())));

        let scalar_checks = data_keys
            .iter()
            .filter(|(key, _)| key_family(key) == Some(b"\x11".as_slice()))
            .all(|(key, _)| !has_key_component(key, b"data"));
        let time_series_checks = data_keys
            .iter()
            .filter(|(key, _)| key_family(key) == Some(b"\x15".as_slice()))
            .all(|(key, _)| !has_key_component(key, b"data"));
        let batch_checks = data_keys
            .iter()
            .filter(|(key, _)| key_family(key) == Some(b"\x17".as_slice()))
            .all(|(key, _)| {
                !has_key_component(key, b"metadata") && !has_key_component(key, b"segment")
            });
        let store_checks = data_keys
            .iter()
            .filter(|(key, _)| key_family(key) == Some(b"\x18".as_slice()))
            .all(|(key, _)| {
                !has_key_component(key, b"row")
                    && !has_key_component(key, b"deleted")
                    && !has_key_component(key, b"field")
            });
        let graph_checks = data_keys
            .iter()
            .filter(|(key, _)| key_family(key) == Some(b"\x16".as_slice()))
            .all(|(key, _)| !has_key_component(key, b"out") && !has_key_component(key, b"in"));
        let fulltext_checks = data_keys
            .iter()
            .filter(|(key, _)| key_family(key) == Some(b"\x12".as_slice()))
            .all(|(key, _)| {
                let has_legacy_segment = has_key_component(key, b"metadata")
                    || has_key_component(key, b"manifest")
                    || has_key_component(key, b"postings")
                    || has_key_component(key, b"documents");
                let has_compact_marker = has_key_component(key, b"\x01")
                    || has_key_component(key, b"\x02")
                    || has_key_component(key, b"\x03")
                    || has_key_component(key, b"\x04");
                !has_legacy_segment && has_compact_marker
            });
        assert!(scalar_checks);
        assert!(time_series_checks);
        assert!(batch_checks);
        assert!(store_checks);
        assert!(graph_checks);
        assert!(fulltext_checks);
    }

    fn key_family(raw: &[u8]) -> Option<&[u8]> {
        raw.split(|byte| *byte == LexKey::SEPARATOR).nth(2)
    }

    fn has_key_component(raw: &[u8], target: &[u8]) -> bool {
        raw.split(|byte| *byte == LexKey::SEPARATOR)
            .any(|part| part == target)
    }

    const SCALAR_INDEX_QUERIES: &[&str] = &[
    "CREATE TABLE keyseg_row (tenant TEXT, event_at TIMESTAMP, title TEXT, email TEXT)",
    "INSERT INTO keyseg_row (tenant, event_at, title, email) VALUES ('acme', '2026-01-01T00:00:00Z', 'alpha', 'a@example.com')",
    "CREATE INDEX keyseg_scalar_idx ON keyseg_row USING btree (email)",
    "CREATE UNIQUE INDEX keyseg_unique_tenant_idx ON keyseg_row USING btree (tenant)",
    "CREATE INDEX keyseg_time_series_idx ON keyseg_row USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
    "CREATE INDEX keyseg_column_idx ON keyseg_row USING column (title) WITH (segment_size = 1)",
    "SELECT title FROM keyseg_row WHERE title = 'alpha'",
    "CREATE TABLE keyseg_column_store (k TEXT, payload TEXT) WITH (storage = column_store)",
    "INSERT INTO keyseg_column_store (k, payload) VALUES ('r1', 'keep'), ('r2', 'delete')",
    "SELECT payload FROM keyseg_column_store WHERE k = 'r1'",
    "DELETE FROM keyseg_column_store WHERE k = 'r2'",
];

    const GRAPH_QUERIES: &[&str] = &[
    "CREATE GRAPH keyseg_graph (NODES (label TEXT), EDGES (source TEXT))",
    "INSERT INTO keyseg_graph_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice')",
    "INSERT INTO keyseg_graph_nodes (node_type, node_id, label) VALUES ('person', 'bob', 'Bob')",
    "INSERT INTO keyseg_graph_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
];

    const FULLTEXT_INDEX_QUERIES: &[&str] = &[
    "CREATE TABLE keyseg_fulltext (title TEXT, body TEXT)",
    "CREATE INDEX keyseg_fulltext_idx ON keyseg_fulltext USING fulltext (body)",
    "INSERT INTO keyseg_fulltext (title, body) VALUES ('first', 'alpha beta'), ('second', 'alpha gamma')",
];
}

// Formerly tests/midge_baseline_database_families.rs.
mod midge_baseline_database_families {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema};

    use super::support_executor as support;
    use support::data_dir;

    fn schema() -> Schema {
        Schema {
            fields: vec![FieldSchema {
                name: "value".to_string(),
                data_type: DataType::Text,
                nullable: false,
            }],
        }
    }

    #[test]
    fn should_route_each_database_to_a_stable_opaque_family() {
        // Arrange
        let path = data_dir("routing");
        let first_mapping = {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            seed_duplicate_database_rows(&cassie);
            assert_database_family_layout(&cassie);
            assert_isolated_rows(&cassie);
            database_family_mapping(&cassie)
        };

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restarted startup");
        let mapping = database_family_mapping(&restarted);

        // Assert
        assert_eq!(mapping, first_mapping);
        assert_isolated_rows(&restarted);
        assert!(restarted
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:")
            .expect("compat scan")
            .is_empty());

        let _ = std::fs::remove_dir_all(path);
    }

    fn seed_duplicate_database_rows(cassie: &Cassie) {
        cassie
            .midge
            .create_database("analytics", None)
            .expect("create database");
        let primary = canonical_relation_name("postgres", "public", "docs");
        let secondary = canonical_relation_name("analytics", "public", "docs");
        cassie
            .midge
            .create_collection(&primary, schema())
            .expect("primary schema");
        cassie
            .midge
            .create_collection(&secondary, schema())
            .expect("secondary schema");
        cassie
            .midge
            .put_document(
                &primary,
                Some("same-id".to_string()),
                serde_json::json!({"value": "one"}),
            )
            .expect("primary row");
        cassie
            .midge
            .put_document(
                &secondary,
                Some("same-id".to_string()),
                serde_json::json!({"value": "two"}),
            )
            .expect("secondary row");
    }

    fn database_family_mapping(cassie: &Cassie) -> (String, String) {
        let databases = cassie.midge.list_databases().expect("databases");
        (
            databases
                .iter()
                .find(|entry| entry.name == "postgres")
                .expect("postgres")
                .physical_family
                .clone(),
            databases
                .iter()
                .find(|entry| entry.name == "analytics")
                .expect("analytics")
                .physical_family
                .clone(),
        )
    }

    fn assert_database_family_layout(cassie: &Cassie) {
        let databases = cassie.midge.list_databases().expect("databases");
        let postgres = databases
            .iter()
            .find(|entry| entry.name == "postgres")
            .expect("postgres");
        let analytics = databases
            .iter()
            .find(|entry| entry.name == "analytics")
            .expect("analytics");
        assert_ne!(postgres.physical_family, analytics.physical_family);
        assert!(!postgres.physical_family.eq_ignore_ascii_case("default"));
        assert!(!postgres.physical_family.eq_ignore_ascii_case("cf1"));
        assert!(!postgres.physical_family.eq_ignore_ascii_case("cf2"));

        for database in ["postgres", "analytics"] {
            let rows = cassie
                .midge
                .raw_scan_prefix_database(database, b"")
                .expect("database data scan");
            assert!(rows.iter().any(|(key, _)| !key
                .windows(database.len())
                .any(|window| window == database.as_bytes())));
        }
    }

    fn assert_isolated_rows(cassie: &Cassie) {
        for (database, value) in [("postgres", "one"), ("analytics", "two")] {
            let collection = canonical_relation_name(database, "public", "docs");
            assert_eq!(
                cassie
                    .midge
                    .get_document(&collection, "same-id")
                    .expect("row read")
                    .expect("row")
                    .payload["value"],
                value
            );
        }
    }
}

// Formerly tests/midge_error_paths.rs.
mod midge_error_paths {
    use cassie::app::CassieError;
    use cntryl_midge::MidgeError;

    #[test]
    fn should_map_write_stall_to_retryable_storage_error() {
        // Arrange
        let error = MidgeError::WriteStall("temporary write stall".to_string());

        // Act
        let mapped = CassieError::from(error);

        // Assert
        assert!(matches!(mapped, CassieError::StorageRetryable(_)));
    }

    #[test]
    fn should_map_fenced_write_to_retryable_storage_error() {
        // Arrange
        let error = MidgeError::Fenced("writer fenced".to_string());

        // Act
        let mapped = CassieError::from(error);

        // Assert
        assert!(matches!(mapped, CassieError::StorageRetryable(_)));
    }

    #[test]
    fn should_map_write_conflict_to_retryable_storage_error() {
        // Arrange
        let error = MidgeError::WriteConflict("conflict on overlapping writes".to_string());

        // Act
        let mapped = CassieError::from(error);

        // Assert
        assert!(matches!(mapped, CassieError::StorageRetryable(_)));
    }

    #[test]
    fn should_map_invalid_argument_family_error_to_missing_family() {
        // Arrange
        let error = MidgeError::InvalidArgument("column family 999 does not exist".to_string());

        // Act
        let mapped = CassieError::from(error);

        // Assert
        assert!(matches!(mapped, CassieError::StorageMissingFamily(_)));
    }
}

// Formerly tests/midge_layout_bootstrap.rs.
mod midge_layout_bootstrap {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::ProjectionRebuildState;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::midge::adapter::{RowDecode, StorageFamily, StorageLayout};
    use cassie::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::TransactionMode;
    use std::path::PathBuf;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::data_dir;

    fn without_fallback() {
        std::env::remove_var("CASSIE_STORAGE_MODE");
    }

    fn normalize_family_ids(layout: &StorageLayout) -> (u32, u32, u32) {
        (layout.schema.id(), layout.data.id(), layout.temp.id())
    }

    fn put_legacy_document(
        cassie: &Cassie,
        collection: &str,
        id: &str,
        payload: &serde_json::Value,
    ) {
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(
            format!("doc:{collection}:{id}").into_bytes(),
            payload.to_string().into_bytes(),
            None,
        )
        .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
    }

    #[test]
    fn should_bootstrap_required_families_idempotently() {
        // Arrange
        let path = data_dir("bootstrap");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let first_state = {
                let cassie = Cassie::new_with_data_dir(&path).unwrap();
                let families = cassie.midge.ensure_families_ready().unwrap().clone();
                (
                    (families.schema.id(), families.data.id(), families.temp.id()),
                    (
                        families.schema.name().to_string(),
                        families.data.name().to_string(),
                        families.temp.name().to_string(),
                    ),
                )
            };

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            let reloaded = restarted.midge.ensure_families_ready().unwrap().clone();
            let second_ids = normalize_family_ids(&reloaded);

            // Assert
            assert_eq!(first_state.1 .0, "cf0");
            assert!(first_state.1 .1.starts_with("db-"));
            assert_eq!(first_state.1 .2, "cf1");
            assert_eq!(second_ids, first_state.0);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_route_schema_data_temp_across_families() {
        // Arrange
        let path = data_dir("routing");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let _ = cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_docs";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
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

            cassie.midge.create_collection(collection, schema).unwrap();
            let doc_id = cassie
                .midge
                .put_document(
                    collection,
                    None,
                    serde_json::json!({"title": "alpha", "embedding": [1.0, 2.0]}),
                )
                .unwrap();

            // Act
            let legacy_row_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"r/")
                .unwrap();
            let legacy_doc_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"doc:")
                .unwrap();
            let legacy_schema_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Schema, b"__cassie__/schema/")
                .unwrap();
            let schema_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Schema, b"")
                .unwrap();
            let temp_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Temp, b"")
                .unwrap();
            let stored = cassie
                .midge
                .get_document(collection, &doc_id)
                .unwrap()
                .expect("stored row should decode");

            // Assert
            assert_eq!(stored.payload["title"], "alpha");
            assert!(legacy_row_entries.is_empty());
            assert!(legacy_doc_entries.is_empty());
            assert!(legacy_schema_entries.is_empty());
            assert!(schema_entries
                .iter()
                .any(|(_, value)| value.as_slice() == b"cassie-midge-layout-v1"));
            assert!(
                temp_entries.is_empty(),
                "temp family should start empty in bootstrap state"
            );

            let mut tx = cassie.midge.temp_tx(TransactionMode::ReadWrite).unwrap();
            tx.put(b"temp:marker".to_vec(), b"1".to_vec(), None)
                .unwrap();
            tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();

            let after_put = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Temp, b"temp:")
                .unwrap();
            assert_eq!(after_put.len(), 1);

            cassie.midge.clear_temp_family().unwrap();
            let after_cleanup = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Temp, b"")
                .unwrap();
            assert!(after_cleanup.is_empty(), "cf2 should support cleanup");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_transactions_that_include_schema_plus_data_families() {
        // Arrange
        let path = data_dir("mixed_family_reject");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            // Act
            let result = cassie.midge.begin_families_tx(
                &[StorageFamily::Schema, StorageFamily::Data],
                TransactionMode::ReadWrite,
            );

            // Assert
            assert!(result.is_err());
            let error = match result {
                Ok(_) => panic!("expected mixed-family transaction to be rejected"),
                Err(error) => error.to_string(),
            };
            assert!(
                error.contains("cannot open a transaction across schema and data families"),
                "unexpected error: {error}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_v1_data_prefix_after_reopen() {
        // Arrange
        let path = data_dir("v1_data_reject");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            {
                let cassie = Cassie::new_with_data_dir(&path).unwrap();
                cassie.midge.ensure_families_ready().unwrap();
                let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
                tx.put(b"doc:legacy:1".to_vec(), b"{}".to_vec(), None)
                    .unwrap();
                tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
            }

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            let result = restarted.startup();

            // Assert
            let error = result.expect_err("v1 data prefix should be rejected");
            assert!(
                error
                    .to_string()
                    .contains("incompatible cassie-midge-layout-v1 storage layout"),
                "unexpected error: {error}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_older_layout_marker() {
        // Arrange
        let path = data_dir("v4_layout_marker_reject");
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();
            let marker_key = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Schema, b"")
                .unwrap()
                .into_iter()
                .find_map(|(key, value)| {
                    (value.as_slice() == b"cassie-midge-layout-v1").then_some(key)
                })
                .expect("baseline layout marker key");
            let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite).unwrap();
            tx.put(marker_key, b"cassie-midge-lexkey-v4".to_vec(), None)
                .unwrap();
            tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        let error = restarted
            .startup()
            .expect_err("v4 layout marker should be rejected");

        // Assert
        let diagnostic = error.to_string();
        assert!(
            diagnostic.contains("found marker 'cassie-midge-lexkey-v4'"),
            "unexpected error: {diagnostic}"
        );
        assert!(
            diagnostic.contains("expected baseline marker 'cassie-midge-layout-v1'"),
            "unexpected error: {diagnostic}"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bootstrap_via_startup_path() {
        // Arrange
        let path = data_dir("bootstrap_startup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();

            // Act
            cassie.startup().unwrap();
            let layout = cassie.midge.ensure_families_ready().unwrap();

            // Assert
            assert_eq!(layout.schema.name(), "cf0");
            assert!(layout.data.name().starts_with("db-"));
            assert_eq!(layout.temp.name(), "cf1");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_preserve_temp_family_during_startup() {
        // Arrange
        let path = data_dir("startup_temp_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let mut tx = cassie
                .midge
                .temp_tx(cntryl_midge::TransactionMode::ReadWrite)
                .unwrap();
            tx.put(b"temp_marker".to_vec(), b"keep-me".to_vec(), None)
                .unwrap();
            tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();

            // Act
            cassie.startup().unwrap();
            let entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Temp, b"temp_")
                .unwrap();

            // Assert
            assert_eq!(entries.len(), 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_cassie_metadata_off_default_family() {
        // Arrange
        let path = data_dir("default_family_guard");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_default_guard";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
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

            cassie.midge.create_collection(collection, schema).unwrap();
            let _ = cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-default-guard".to_string()),
                    serde_json::json!({"title": "alpha", "embedding": [1.0, 2.0]}),
                )
                .unwrap();

            // Act
            let default_entries = cassie.midge.raw_scan_prefix_named("default", b"").unwrap();

            // Assert
            for (key, _) in default_entries {
                let key = String::from_utf8_lossy(&key);
                assert!(
                    !key.starts_with("__cassie__/")
                        && !key.starts_with("doc:")
                        && !key.starts_with("r/"),
                    "no Cassie-managed keys should be stored in default family"
                );
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_make_startup_idempotent_when_reinvoked() {
        // Arrange
        without_fallback();
        let path = data_dir("startup_idempotent");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();

            // Act
            cassie.startup().unwrap();
            let families_first = cassie.midge.ensure_families_ready().unwrap().clone();
            cassie.startup().unwrap();
            let families_second = cassie.midge.ensure_families_ready().unwrap().clone();

            // Assert
            assert_eq!(families_first.schema.id(), families_second.schema.id());
            assert_eq!(families_first.data.id(), families_second.data.id());
            assert_eq!(families_first.temp.id(), families_second.temp.id());
            assert_eq!(families_first.schema.name(), families_second.schema.name());
            assert_eq!(families_first.data.name(), families_second.data.name());
            assert_eq!(families_first.temp.name(), families_second.temp.name());

            let entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Temp, b"")
                .unwrap();
            assert!(entries.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_fail_startup_when_data_dir_is_not_writable_directory() {
        // Arrange
        without_fallback();
        let base_path = PathBuf::from(data_dir("invalid_parent"));
        let _ = std::fs::remove_file(&base_path);
        std::fs::write(&base_path, "locked").unwrap();
        let path = format!("{}/child", base_path.to_string_lossy());

        // Act
        let created = Cassie::new_with_data_dir(&path);

        // Assert
        assert!(created.is_err());

        let _ = std::fs::remove_file(&base_path);
    }
}

// Formerly tests/midge_legacy_migration.rs.
mod midge_legacy_migration {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_executor as support;
    use support::data_dir;

    fn put_legacy_data_key(path: &str, key: &[u8]) {
        let cassie = Cassie::new_with_data_dir(path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(key.to_vec(), b"{}".to_vec(), None).unwrap();
        tx.commit(WriteOptions::sync()).unwrap();
    }

    #[test]
    fn should_reject_legacy_doc_prefix_on_reopen() {
        // Arrange
        let path = data_dir("doc_prefix");
        put_legacy_data_key(&path, b"doc:legacy:1");

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        let result = restarted.startup();

        // Assert
        let error = result.expect_err("legacy doc prefix should be rejected");
        assert!(
            error
                .to_string()
                .contains("incompatible cassie-midge-layout-v1 storage layout"),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_legacy_row_prefix_on_reopen() {
        // Arrange
        let path = data_dir("row_prefix");
        put_legacy_data_key(&path, b"r/legacy/1");

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        let result = restarted.startup();

        // Assert
        let error = result.expect_err("legacy row prefix should be rejected");
        assert!(
            error
                .to_string()
                .contains("incompatible cassie-midge-layout-v1 storage layout"),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_ignore_legacy_doc_key_written_after_bootstrap() {
        // Arrange
        let path = data_dir("post_bootstrap_doc");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = canonical_relation_name("postgres", "public", "legacy_break");
        cassie.midge.ensure_families_ready().unwrap();
        cassie
            .midge
            .create_collection(
                &collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .unwrap();
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(
            b"doc:legacy_break:stale".to_vec(),
            serde_json::json!({"title": "stale"})
                .to_string()
                .into_bytes(),
            None,
        )
        .unwrap();
        tx.commit(WriteOptions::sync()).unwrap();

        // Act
        let scanned = cassie.midge.scan_documents(&collection).unwrap();
        let legacy_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:")
            .unwrap();

        // Assert
        assert!(scanned.is_empty());
        assert_eq!(legacy_entries.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/midge_metadata_stats.rs.
mod midge_metadata_stats {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{CollectionMeta, CollectionStorageMode, IndexKind, IndexMeta};
    use cassie::catalog::{ProjectionFreshness, ProjectionRebuildState};
    use cassie::midge::adapter::{RowDecode, StorageFamily, StorageLayout};
    use cassie::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::TransactionMode;
    use std::path::PathBuf;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::data_dir;

    fn without_fallback() {
        std::env::remove_var("CASSIE_STORAGE_MODE");
    }

    fn normalize_family_ids(layout: &StorageLayout) -> (u32, u32, u32) {
        (layout.schema.id(), layout.data.id(), layout.temp.id())
    }

    fn put_legacy_document(
        cassie: &Cassie,
        collection: &str,
        id: &str,
        payload: &serde_json::Value,
    ) {
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(
            format!("doc:{collection}:{id}").into_bytes(),
            payload.to_string().into_bytes(),
            None,
        )
        .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
    }

    #[test]
    fn should_restore_cardinality_stats_after_restart() {
        // Arrange
        let path = data_dir("cardinality_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_cardinality_restart";
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
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
                    },
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha", "body": "bravo"}),
                )
                .unwrap();
            cassie
                .midge
                .put_index(&IndexMeta {
                    collection: collection.to_string(),
                    name: "idx_title".to_string(),
                    field: "title".to_string(),
                    fields: vec!["title".to_string()],
                    expressions: Vec::new(),
                    include_fields: Vec::new(),
                    predicate: None,
                    kind: IndexKind::Scalar,
                    unique: false,
                    options: std::collections::BTreeMap::default(),
                })
                .unwrap();
            cassie
                .midge
                .rebuild_cardinality_stats_for_collection(collection)
                .unwrap();

            // Act
            drop(cassie);
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.midge.ensure_families_ready().unwrap();
            let stats = restarted
                .midge
                .get_cardinality_stats(collection)
                .unwrap()
                .expect("stored cardinality stats");

            // Assert
            assert!(stats.hydrated);
            assert_eq!(stats.row_count, 1);
            assert_eq!(
                stats
                    .indexes
                    .get("scalar:idx_title")
                    .map(|entry| entry.cardinality),
                Some(1)
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_rebuild_field_cardinality_stats() {
        // Arrange
        let path = data_dir("field_cardinality");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();
            let collection = "cf_layout_field_cardinality";
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
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
                    },
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha", "body": null}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-2".to_string()),
                    serde_json::json!({"title": "beta", "body": "two"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-3".to_string()),
                    serde_json::json!({"body": "three"}),
                )
                .unwrap();

            // Act
            let stats = cassie
                .midge
                .rebuild_cardinality_stats_for_collection(collection)
                .unwrap();

            // Assert
            let title = stats.fields.get("title").expect("title field stats");
            assert_eq!(title.non_null_count, 2);
            assert_eq!(title.missing_count, 1);
            assert_eq!(title.distinct_count, 2);
            assert_eq!(title.min_value.as_deref(), Some("\"alpha\""));
            assert_eq!(title.max_value.as_deref(), Some("\"beta\""));
            let body = stats.fields.get("body").expect("body field stats");
            assert_eq!(body.non_null_count, 2);
            assert_eq!(body.null_count, 1);
            assert_eq!(body.distinct_count, 2);
            assert_eq!(title.sample_count, 3);
            assert_eq!(title.confidence, 66);
            assert_eq!(title.histogram_buckets.len(), 2);
            assert_eq!(title.heavy_hitters[0].value, "\"alpha\"");
            assert_eq!(title.heavy_hitters[0].count, 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_move_cleanup_cardinality_stats_on_collection_rename_drop() {
        // Arrange
        let path = data_dir("cardinality_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let current = "cf_layout_cardinality_cleanup";
            let next = "cf_layout_cardinality_cleanup_next";
            cassie
                .midge
                .create_collection(
                    current,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    current,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .rebuild_cardinality_stats_for_collection(current)
                .unwrap();

            // Act
            cassie.midge.rename_collection(current, next).unwrap();
            let renamed_stats = cassie
                .midge
                .get_cardinality_stats(next)
                .unwrap()
                .expect("renamed stats");
            let old_stats = cassie.midge.get_cardinality_stats(current).unwrap();
            cassie.midge.drop_collection(next).unwrap();
            let dropped_stats = cassie.midge.get_cardinality_stats(next).unwrap();

            // Assert
            assert_eq!(renamed_stats.row_count, 1);
            assert!(old_stats.is_none());
            assert!(dropped_stats.is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_cleanup_column_store_keys_after_collection_rename_then_drop() {
        // Arrange
        let path = data_dir("column_store_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();
            let collection = "cf_layout_column_store_cleanup";
            let renamed = "cf_layout_column_store_cleanup_archive";
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
            let metadata = CollectionMeta::new_with_storage_mode(
                collection,
                None,
                CollectionStorageMode::ColumnStore,
            );
            cassie
                .midge
                .create_collection_with_meta(collection, &schema, &metadata)
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha", "score": 7}),
                )
                .unwrap();

            assert!(cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"__cassie__/column-store/v1/")
                .unwrap()
                .is_empty());

            // Act
            cassie.midge.rename_collection(collection, renamed).unwrap();
            let metadata = cassie
                .midge
                .collection_metadata(renamed)
                .unwrap()
                .expect("collection metadata");
            let moved = cassie.midge.get_document(renamed, "doc-1").unwrap();
            cassie.midge.drop_collection(renamed).unwrap();

            // Assert
            assert_eq!(metadata.storage_mode, CollectionStorageMode::ColumnStore);
            assert!(moved.is_some());
            assert!(cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"__cassie__/column-store/v1/")
                .unwrap()
                .is_empty());
            assert!(cassie.midge.collection_metadata(renamed).unwrap().is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_store_column_values_under_numeric_ids_without_json_wrappers() {
        // Arrange
        let path = data_dir("column_store_compact_layout");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.ensure_families_ready().unwrap();
        let collection = "column_store_compact_layout";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        let metadata = CollectionMeta::new_with_storage_mode(
            collection,
            None,
            CollectionStorageMode::ColumnStore,
        );
        cassie
            .midge
            .create_collection_with_meta(collection, &schema, &metadata)
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("row-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();

        // Act
        let prefix = cassie
            .midge
            .column_store_prefix_for_diagnostics(collection)
            .unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .unwrap();

        // Assert
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|(key, _)| !key
            .windows(collection.len())
            .any(|window| window == collection.as_bytes())));
        assert!(entries
            .iter()
            .all(|(_, value)| value.first() != Some(&b'"')));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_persist_projection_metadata_in_schema_family() {
        // Arrange
        let path = data_dir("projection_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_projection_metadata";

            // Act
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .unwrap();
            let metadata = cassie
                .midge
                .projection_metadata(collection)
                .unwrap()
                .expect("projection metadata should exist");
            let legacy_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Schema, b"__cassie__/projection/")
                .unwrap();

            // Assert
            assert_eq!(metadata.collection, collection);
            assert_eq!(metadata.schema_version, 1);
            assert_eq!(metadata.offset, 0);
            assert_eq!(metadata.lag, 0);
            assert_eq!(metadata.rebuild_state, ProjectionRebuildState::Idle);
            assert!(legacy_entries.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_projection_metadata_during_startup() {
        // Arrange
        let path = data_dir("projection_metadata_hydrate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            cassie
                .midge
                .create_collection(
                    "hydrated_projection_metadata",
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .unwrap();

            // Act
            cassie.startup().unwrap();
            let metadata = cassie
                .catalog
                .get_projection_metadata("hydrated_projection_metadata")
                .expect("projection metadata should hydrate");

            // Assert
            assert_eq!(metadata.collection, "hydrated_projection_metadata");
            assert_eq!(metadata.schema_version, 1);
            assert_eq!(metadata.rebuild_state, ProjectionRebuildState::Idle);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_legacy_projection_metadata_on_reopen() {
        // Arrange
        let path = data_dir("projection_metadata_legacy");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            {
                let cassie = Cassie::new_with_data_dir(&path).unwrap();
                cassie.midge.ensure_families_ready().unwrap();
                let legacy = serde_json::json!({
                    "collection": "legacy_projection_metadata",
                    "schema_version": 1,
                    "offset": 9,
                    "lag": 2,
                    "rebuild_state": "idle"
                });
                let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite).unwrap();
                tx.put(
                    b"__cassie__/projection/legacy_projection_metadata".to_vec(),
                    legacy.to_string().into_bytes(),
                    None,
                )
                .unwrap();
                tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
            }

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            let result = restarted.startup();

            // Assert
            let error =
                result.expect_err("legacy projection metadata should reject baseline startup");
            assert!(
                error
                    .to_string()
                    .contains("incompatible cassie-midge-layout-v1 storage layout"),
                "unexpected error: {error}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_cleanup_projection_checkpoint_metadata_on_rename_drop() {
        // Arrange
        let path = data_dir("projection_checkpoint_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();
            let current = "projection_checkpoint_cleanup";
            let next = "projection_checkpoint_cleanup_next";
            cassie
                .midge
                .create_collection(
                    current,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .unwrap();
            let mut metadata = cassie
                .midge
                .projection_metadata(current)
                .unwrap()
                .expect("projection metadata");
            metadata.source_identity = Some("orders-stream".to_string());
            metadata.source_checkpoint = Some("checkpoint-7".to_string());
            metadata.last_applied_event_id = Some("event-7".to_string());
            metadata.replay_batch_id = Some("batch-7".to_string());
            metadata.freshness = ProjectionFreshness::Fresh;
            cassie.midge.put_projection_metadata(&metadata).unwrap();

            // Act
            cassie.midge.rename_collection(current, next).unwrap();
            let renamed = cassie
                .midge
                .projection_metadata(next)
                .unwrap()
                .expect("renamed metadata");
            let old = cassie.midge.projection_metadata(current).unwrap();
            cassie.midge.drop_collection(next).unwrap();
            let dropped = cassie.midge.projection_metadata(next).unwrap();

            // Assert
            assert_eq!(renamed.collection, next);
            assert_eq!(renamed.source_identity.as_deref(), Some("orders-stream"));
            assert_eq!(renamed.source_checkpoint.as_deref(), Some("checkpoint-7"));
            assert_eq!(renamed.last_applied_event_id.as_deref(), Some("event-7"));
            assert_eq!(renamed.replay_batch_id.as_deref(), Some("batch-7"));
            assert_eq!(renamed.freshness, ProjectionFreshness::Fresh);
            assert!(old.is_none());
            assert!(dropped.is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/midge_namespace_hydration.rs.
mod midge_namespace_hydration {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::canonical_schema_name;
    use cassie::catalog::ProjectionRebuildState;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::midge::adapter::{RowDecode, StorageFamily, StorageLayout};
    use cassie::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::TransactionMode;
    use std::path::PathBuf;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::data_dir;

    fn without_fallback() {
        std::env::remove_var("CASSIE_STORAGE_MODE");
    }

    fn normalize_family_ids(layout: &StorageLayout) -> (u32, u32, u32) {
        (layout.schema.id(), layout.data.id(), layout.temp.id())
    }

    fn put_legacy_document(
        cassie: &Cassie,
        collection: &str,
        id: &str,
        payload: &serde_json::Value,
    ) {
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(
            format!("doc:{collection}:{id}").into_bytes(),
            payload.to_string().into_bytes(),
            None,
        )
        .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
    }

    #[test]
    fn should_reject_legacy_collections_index_on_reopen() {
        // Arrange
        let path = data_dir("schema_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "fallback_collection";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie.midge.create_collection(collection, schema).unwrap();

            {
                let mut tx = cassie.midge.schema_tx(TransactionMode::ReadWrite).unwrap();
                tx.put(
                    b"__cassie__/collections".to_vec(),
                    serde_json::to_vec(&vec![collection]).unwrap(),
                    None,
                )
                .unwrap();
                tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
            }

            drop(cassie);

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            let result = restarted.startup();

            // Assert
            let error = result.expect_err("legacy collections index should be rejected");
            assert!(
                error
                    .to_string()
                    .contains("incompatible cassie-midge-layout-v1 storage layout"),
                "unexpected error: {error}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_refresh_in_memory_catalog_during_startup() {
        // Arrange
        let path = data_dir("startup_catalog_refresh");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            cassie
                .midge
                .create_collection(
                    "hydrated_collection",
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .unwrap();

            cassie.register_collection(
                "ghost_collection",
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            );

            // Act
            cassie.startup().unwrap();
            let collections = cassie
                .catalog
                .list_collections()
                .into_iter()
                .map(|collection| collection.name)
                .collect::<Vec<_>>();

            // Assert
            assert!(collections
                .iter()
                .any(|value| value == "hydrated_collection"));
            assert!(!collections.iter().any(|value| value == "ghost_collection"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_namespace_catalog_from_schema_family() {
        // Arrange
        let path = data_dir("schema_namespace_hydrate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let namespace = canonical_schema_name("postgres", "reporting");

            cassie.midge.create_namespace(&namespace).unwrap();

            drop(cassie);

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let namespaces = restarted
                .catalog
                .list_namespaces()
                .into_iter()
                .map(|namespace| namespace.name)
                .collect::<Vec<_>>();

            // Assert
            assert!(namespaces.iter().any(|name| name == &namespace));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_renamed_namespace_catalog_from_schema_family() {
        // Arrange
        let path = data_dir("schema_namespace_rename_hydrate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let current = canonical_schema_name("postgres", "reporting");
            let next = canonical_schema_name("postgres", "reporting_archive");

            cassie.midge.create_namespace(&current).unwrap();
            cassie.midge.rename_namespace(&current, &next).unwrap();

            drop(cassie);

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let namespaces = restarted
                .catalog
                .list_namespaces()
                .into_iter()
                .map(|namespace| namespace.name)
                .collect::<Vec<_>>();

            // Assert
            assert!(!namespaces.iter().any(|name| name == &current));
            assert!(namespaces.iter().any(|name| name == &next));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_dropped_namespace_catalog_from_schema_family() {
        // Arrange
        let path = data_dir("schema_namespace_drop_hydrate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let namespace = canonical_schema_name("postgres", "reporting");

            cassie.midge.create_namespace(&namespace).unwrap();
            cassie.midge.drop_namespace(&namespace).unwrap();

            drop(cassie);

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let namespaces = restarted
                .catalog
                .list_namespaces()
                .into_iter()
                .map(|namespace| namespace.name)
                .collect::<Vec<_>>();

            // Assert
            assert!(!namespaces.iter().any(|name| name == &namespace));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/midge_row_blob_layout.rs.
mod midge_row_blob_layout {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::ProjectionRebuildState;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::midge::adapter::{RowDecode, StorageFamily, StorageLayout};
    use cassie::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::TransactionMode;
    use std::path::PathBuf;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::data_dir;

    fn without_fallback() {
        std::env::remove_var("CASSIE_STORAGE_MODE");
    }

    fn normalize_family_ids(layout: &StorageLayout) -> (u32, u32, u32) {
        (layout.schema.id(), layout.data.id(), layout.temp.id())
    }

    fn put_legacy_document(
        cassie: &Cassie,
        collection: &str,
        id: &str,
        payload: &serde_json::Value,
    ) {
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(
            format!("doc:{collection}:{id}").into_bytes(),
            payload.to_string().into_bytes(),
            None,
        )
        .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
    }

    #[test]
    fn should_store_rows_as_field_id_blobs_in_data_family() {
        // Arrange
        let path = data_dir("row_blob_data");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_row_blob";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
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

            cassie.midge.create_collection(collection, schema).unwrap();
            let doc_id = cassie
                .midge
                .put_document(
                    collection,
                    None,
                    serde_json::json!({"title": "alpha", "embedding": [1.0, 2.0]}),
                )
                .unwrap();

            // Act
            let legacy_row_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"r/")
                .unwrap();
            let legacy_doc_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"doc:")
                .unwrap();
            let stored = cassie
                .midge
                .get_document(collection, &doc_id)
                .unwrap()
                .expect("stored row should decode");

            // Assert
            assert_eq!(stored.payload["title"], "alpha");
            assert!(legacy_row_entries.is_empty());
            assert!(legacy_doc_entries.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_preserve_monotonic_relation_ids_across_object_lifecycle() {
        // Arrange
        let path = data_dir("relation_ids");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let schema = Schema { fields: Vec::new() };
            cassie
                .midge
                .create_collection("relation_id_first", schema.clone())
                .unwrap();
            let first = cassie
                .midge
                .collection_metadata("relation_id_first")
                .unwrap()
                .expect("first metadata")
                .storage_id;

            // Act
            cassie
                .midge
                .rename_collection("relation_id_first", "relation_id_renamed")
                .unwrap();
            let renamed = cassie
                .midge
                .collection_metadata("relation_id_renamed")
                .unwrap()
                .expect("renamed metadata")
                .storage_id;
            cassie.midge.drop_collection("relation_id_renamed").unwrap();
            cassie
                .midge
                .create_collection("relation_id_second", schema)
                .unwrap();
            let second = cassie
                .midge
                .collection_metadata("relation_id_second")
                .unwrap()
                .expect("second metadata")
                .storage_id;

            // Assert
            assert!(first > 0);
            assert_eq!(renamed, first);
            assert!(second > first);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reduce_fixed_query_hot_row_fixture_by_at_least_twenty_five_percent() {
        // Arrange
        let path = data_dir("query_hot_bytes");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "public.customer_activity_observation_records";
            let field_names = [
                "customer_account_external_identifier",
                "activity_observation_category_name",
                "activity_observation_source_system",
                "activity_observation_region_code",
                "activity_observation_status_description",
                "activity_observation_correlation_identifier",
            ];
            let schema = Schema {
                fields: field_names
                    .iter()
                    .map(|name| FieldSchema {
                        name: (*name).to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    })
                    .collect(),
            };
            let payload = serde_json::Value::Object(
                field_names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        (
                            (*name).to_string(),
                            serde_json::json!(format!("value-{index}")),
                        )
                    })
                    .collect(),
            );
            cassie.midge.create_collection(collection, schema).unwrap();
            let id = "fixed-row-00000001";
            cassie
                .midge
                .put_document(collection, Some(id.to_string()), payload.clone())
                .unwrap();

            // Act
            let (key, value) = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"")
                .unwrap()
                .into_iter()
                .find(|(_, value)| value.starts_with(b"CRB2"))
                .expect("binary row record");
            let baseline_key_bytes =
                b"cassie\0lexkey\0v5\0row\0".len() + collection.len() + id.len() + 2;
            let baseline_value_bytes = serde_json::to_vec(&payload).unwrap().len();
            let baseline_bytes = baseline_key_bytes + baseline_value_bytes;
            let compact_bytes = key.len() + value.len();

            // Assert
            assert!(value.starts_with(b"CRB2"));
            assert!(!key
                .windows(collection.len())
                .any(|window| window == collection.as_bytes()));
            assert!(compact_bytes.saturating_mul(4) <= baseline_bytes.saturating_mul(3));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_preserve_retired_field_ids_in_row_schema_metadata() {
        // Arrange
        let path = data_dir("row_schema_ids");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_row_schema";
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
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
                    },
                )
                .unwrap();
            cassie
                .midge
                .alter_collection_drop_column(collection, "title")
                .unwrap();

            // Act
            cassie
                .midge
                .alter_collection_add_column(
                    collection,
                    FieldSchema {
                        name: "status".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                )
                .unwrap();
            let schema_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Schema, b"")
                .unwrap();

            // Assert
            let row_schema = schema_entries
                .iter()
                .filter_map(|(_, raw)| serde_json::from_slice::<serde_json::Value>(raw).ok())
                .find(|value| value["next_field_id"] == 4 && value["schema_version"] == 3)
                .expect("row schema metadata should be persisted");
            assert_eq!(row_schema["schema_version"], 3);
            assert_eq!(row_schema["next_field_id"], 4);

            let fields = row_schema["fields"].as_array().expect("fields array");
            let title = fields
                .iter()
                .find(|field| field["name"] == "title")
                .expect("title field metadata should remain retired");
            let body = fields
                .iter()
                .find(|field| field["name"] == "body")
                .expect("body field metadata should remain active");
            let status = fields
                .iter()
                .find(|field| field["name"] == "status")
                .expect("status field metadata should be added");

            assert_eq!(title["field_id"], 1);
            assert_eq!(title["retired"], true);
            assert_eq!(body["field_id"], 2);
            assert_eq!(body["retired"], false);
            assert_eq!(status["field_id"], 3);
            assert_eq!(status["retired"], false);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_ignore_legacy_document_rows_after_layout_break() {
        // Arrange
        let path = data_dir("legacy_scan");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_legacy_scan";
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .unwrap();
            put_legacy_document(
                &cassie,
                collection,
                "legacy-1",
                &serde_json::json!({"title": "legacy"}),
            );

            // Act
            let documents = cassie.midge.scan_documents(collection).unwrap();

            // Assert
            assert!(documents.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_iterate_rebuild_rows_across_storage_sources() {
        // Arrange
        let path = data_dir("row_rebuild_iter");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_rebuild_iter";
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("dupe-1".to_string()),
                    serde_json::json!({"title": "row"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("row-1".to_string()),
                    serde_json::json!({"title": "fresh"}),
                )
                .unwrap();
            // Act
            let rows = cassie
                .midge
                .scan_rows_for_rebuild(collection, RowDecode::Full)
                .unwrap();

            // Assert
            assert_eq!(rows.len(), 2);
            assert_eq!(
                rows.iter()
                    .find(|row| row.id == "dupe-1")
                    .expect("duplicate row id")
                    .payload["title"],
                "row"
            );
            assert!(rows.iter().any(|row| row.id == "row-1"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_project_active_fields_for_rebuild_rows() {
        // Arrange
        let path = data_dir("row_rebuild_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.ensure_families_ready().unwrap();

            let collection = "cf_layout_rebuild_projection";
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
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
                            FieldSchema {
                                name: "status".to_string(),
                                data_type: DataType::Text,
                                nullable: true,
                            },
                        ],
                    },
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("row-1".to_string()),
                    serde_json::json!({
                        "title": "alpha",
                        "body": "retired",
                        "status": "ready",
                    }),
                )
                .unwrap();
            cassie
                .midge
                .alter_collection_drop_column(collection, "body")
                .unwrap();

            // Act
            let rows = cassie
                .midge
                .scan_rows_for_rebuild(
                    collection,
                    RowDecode::Projected(vec!["title".to_string(), "body".to_string()]),
                )
                .unwrap();

            // Assert
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].payload, serde_json::json!({"title": "alpha"}));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/operational_ownership.rs.
mod operational_ownership {
    use cassie::app::Cassie;
    use cassie::catalog::{OperationalAssignmentMeta, OperationalAssignmentState};
    use cassie::types::Value;

    use super::support_data_dir as data_dir;
    use super::support_local_storage as local_storage;
    use data_dir::data_dir;
    use local_storage::use_local_storage;

    fn assignment(projection_id: &str, tenant: &str) -> OperationalAssignmentMeta {
        OperationalAssignmentMeta {
            assignment_id: format!("{projection_id}-{tenant}"),
            node_id: "node-a".to_string(),
            projection_id: projection_id.to_string(),
            tenant: Some(tenant.to_string()),
            partition_key: Some(format!("{tenant}:0")),
            generation: 7,
            state: OperationalAssignmentState::Claimed,
            routing_hint: Some(format!("local://node-a/{projection_id}/{tenant}")),
            updated_ms: 1_234,
        }
    }

    #[test]
    fn should_persist_operational_assignment_metadata_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("restart");
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
                "CREATE TABLE operational_restart_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .put_operational_assignment(assignment("operational_restart_docs", "tenant-a"))
            .unwrap();
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT node_id, projection_id, tenant, partition_key, generation, state, routing_hint, updated_ms FROM pg_catalog.pg_operational_assignments WHERE assignment_id = 'operational_restart_docs-tenant-a'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("node-a".to_string()),
                Value::String("operational_restart_docs".to_string()),
                Value::String("tenant-a".to_string()),
                Value::String("tenant-a:0".to_string()),
                Value::Int64(7),
                Value::String("claimed".to_string()),
                Value::String("local://node-a/operational_restart_docs/tenant-a".to_string()),
                Value::Int64(1_234),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_not_route_or_filter_queries_from_operational_assignment_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("query_semantics");
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
                "CREATE TABLE operational_query_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO operational_query_docs (tenant_id, title) VALUES ('tenant-a', 'alpha'), ('tenant-b', 'bravo')",
                vec![],
            )
            .unwrap();
        cassie
            .put_operational_assignment(assignment("operational_query_docs", "tenant-a"))
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT tenant_id, title FROM operational_query_docs ORDER BY tenant_id",
                vec![],
            )
            .unwrap();
        let routed_tenant = cassie
            .execute_sql(
                &session,
                "SELECT title FROM operational_query_docs WHERE tenant_id = 'tenant-b'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("tenant-a".to_string()),
                    Value::String("alpha".to_string())
                ],
                vec![
                    Value::String("tenant-b".to_string()),
                    Value::String("bravo".to_string())
                ],
            ]
        );
        assert_eq!(
            routed_tenant.rows,
            vec![vec![Value::String("bravo".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_last_write_wins_on_concurrent_assignment_updates() {
        // Arrange
        use_local_storage();
        let path = data_dir("concurrent_update");
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
                "CREATE TABLE operational_concurrent_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();

        let mut first = assignment("operational_concurrent_docs", "tenant-a");
        first.generation = 1;
        first.state = OperationalAssignmentState::Claimed;
        first.updated_ms = 1_000;
        let mut second = first.clone();
        second.generation = 2;
        second.state = OperationalAssignmentState::Draining;
        second.updated_ms = 2_000;
        let mut third = first.clone();
        third.generation = 3;
        third.state = OperationalAssignmentState::Released;
        third.updated_ms = 3_000;

        // Act: simulate racing writers landing out of submission order but in
        // increasing generation/updated_ms order, matching the assignment
        // lifecycle contract in docs/operational-scale.md.
        cassie.put_operational_assignment(first).unwrap();
        cassie.put_operational_assignment(second).unwrap();
        cassie.put_operational_assignment(third).unwrap();

        let selected = cassie
            .execute_sql(
                &session,
                "SELECT generation, state, updated_ms FROM pg_catalog.pg_operational_assignments WHERE assignment_id = 'operational_concurrent_docs-tenant-a'",
                vec![],
            )
            .unwrap();
        let all_for_assignment = cassie
            .execute_sql(
                &session,
                "SELECT assignment_id FROM pg_catalog.pg_operational_assignments WHERE assignment_id = 'operational_concurrent_docs-tenant-a'",
                vec![],
            )
            .unwrap();

        // Assert: exactly one row survives, holding the last write.
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::Int64(3),
                Value::String("released".to_string()),
                Value::Int64(3_000),
            ]]
        );
        assert_eq!(all_for_assignment.rows.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_bound_assignment_cardinality_to_distinct_assignment_ids() {
        // Arrange
        use_local_storage();
        let path = data_dir("bounded_cardinality");
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
                "CREATE TABLE operational_bounded_docs (tenant_id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();

        // Act: write 5 distinct assignments, each updated 4 times, so 20
        // total put_operational_assignment calls land on 5 distinct keys.
        let tenants = ["tenant-a", "tenant-b", "tenant-c", "tenant-d", "tenant-e"];
        for tenant in tenants {
            for generation in 1..=4 {
                let mut meta = assignment("operational_bounded_docs", tenant);
                meta.generation = generation;
                meta.updated_ms = 1_000 * generation;
                cassie.put_operational_assignment(meta).unwrap();
            }
        }

        let listed = cassie.list_operational_assignments();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT assignment_id FROM pg_catalog.pg_operational_assignments WHERE projection_id = 'operational_bounded_docs'",
                vec![],
            )
            .unwrap();

        // Assert: cardinality tracks distinct assignment_id count, not the
        // number of writes, for both the in-memory catalog and the
        // persisted store.
        assert_eq!(listed.len(), tenants.len());
        assert_eq!(selected.rows.len(), tenants.len());
        assert!(listed.iter().all(|meta| meta.generation == 4));

        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        assert_eq!(restarted.list_operational_assignments().len(), tenants.len());

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/snapshot_restore.rs.
mod snapshot_restore {
    #![allow(unused_imports, dead_code)]

    use cassie::app::{Cassie, CassieSnapshotManifest, CassieSnapshotOptions};
    use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
    use cassie::catalog::canonical_relation_name;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    fn canonical_collection(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    fn seed_replayed_projection(path: &str, table: &str) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {table} (title TEXT, score INT)"),
                    vec![],
                )
                .unwrap();
            let projection = canonical_test_collection(&cassie, table);
            cassie
                .replay_projection_batch(ProjectionReplayBatch {
                    projection: projection.clone(),
                    source_identity: "orders-stream".to_string(),
                    batch_id: "batch-1".to_string(),
                    lag: 0,
                    events: vec![ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-1".to_string(),
                        position: Some(1),
                        document_id: "doc-1".to_string(),
                        payload: Some(serde_json::json!({"title": "alpha", "score": 10})),
                    }],
                })
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    &format!("VERIFY PROJECTION {table} MODE full"),
                    vec![],
                )
                .unwrap();
            drop(cassie);
        });
    }

    #[test]
    fn should_create_snapshot_manifest_with_projection_checkpoint_hash_metadata() {
        // Arrange
        use_local_storage();
        let source = data_dir("snapshot_manifest_source");
        let snapshot = data_dir("snapshot_manifest_bundle");
        seed_replayed_projection(&source, "snapshot_manifest_docs");

        // Act
        let manifest = Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(1_234),
            },
        )
        .unwrap();
        let manifest_path = std::path::Path::new(&snapshot).join("cassie-snapshot-manifest.json");
        let manifest_file: CassieSnapshotManifest =
            serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();

        // Assert
        assert_eq!(manifest, manifest_file);
        assert_eq!(manifest.format_version, 2);
        assert_eq!(manifest.cassie_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(manifest.generated_ms, 1_234);
        assert_eq!(manifest.compatibility_status, "compatible");
        assert_eq!(manifest.midge_data_path, "midge");
        assert!(manifest.schema_epoch >= 1);
        let projection = manifest
            .projections
            .iter()
            .find(|projection| {
                projection.projection_id == canonical_collection("snapshot_manifest_docs")
            })
            .expect("projection manifest");
        assert_eq!(projection.source_identity.as_deref(), Some("orders-stream"));
        assert_eq!(
            projection.source_checkpoint.as_deref(),
            Some("checkpoint-1")
        );
        assert_eq!(projection.source_position, Some(1));
        assert_eq!(projection.hash.algorithm, "cassie-fnv128");
        assert_eq!(projection.hash.digest_length, 16);
        assert!(projection.hash.root_digest.is_some());
        assert_eq!(projection.hash.root_state, "current");

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
    }

    #[test]
    fn should_record_collection_generations_for_snapshot_consistency() {
        // Arrange
        use_local_storage();
        let source = data_dir("snapshot_collection_generations_source");
        let snapshot = data_dir("snapshot_collection_generations_bundle");
        seed_replayed_projection(&source, "snapshot_collection_generations_docs");
        let collection = canonical_collection("snapshot_collection_generations_docs");

        // Act
        let manifest = Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(4_680),
            },
        )
        .expect("create snapshot");
        let source_cassie = Cassie::new_with_data_dir(&source).expect("reopen source");
        source_cassie.startup().expect("start source");

        // Assert
        let generation = source_cassie
            .midge
            .collection_generation(&collection)
            .expect("source generation");
        let recorded = manifest
            .collections
            .iter()
            .find(|entry| entry.collection == collection)
            .expect("collection generation manifest");
        assert_eq!(recorded.generation, generation);
        assert!(manifest.data_epoch > 0);

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
    }

    #[test]
    fn should_restore_snapshot_to_new_data_dir_for_startup_query() {
        // Arrange
        use_local_storage();
        let source = data_dir("snapshot_restore_source");
        let snapshot = data_dir("snapshot_restore_bundle");
        let restored = data_dir("snapshot_restore_restored");
        seed_replayed_projection(&source, "snapshot_restore_docs");
        Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(2_468),
            },
        )
        .unwrap();

        // Act
        let restored_manifest = Cassie::restore_snapshot(&snapshot, &restored).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&restored).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let projection = canonical_test_collection(&cassie, "snapshot_restore_docs");
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM snapshot_restore_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT source_checkpoint, last_applied_event_id, freshness FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(restored_manifest.generated_ms, 2_468);
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(10)]]
        );
        assert_eq!(
            checkpoint.rows,
            vec![vec![
                Value::String("checkpoint-1".to_string()),
                Value::String("event-1".to_string()),
                Value::String("fresh".to_string()),
            ]]
        );

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
        let _ = std::fs::remove_dir_all(restored);
    });
    }

    #[test]
    #[ignore = "restores workflow-selected operational data for a fresh container"]
    fn should_restore_operational_snapshot_selected_by_environment() {
        // Arrange
        let source = std::env::var("CASSIE_OPERATIONAL_SNAPSHOT_SOURCE")
            .expect("CASSIE_OPERATIONAL_SNAPSHOT_SOURCE");
        let snapshot = std::env::var("CASSIE_OPERATIONAL_SNAPSHOT_BUNDLE")
            .expect("CASSIE_OPERATIONAL_SNAPSHOT_BUNDLE");
        let restored = std::env::var("CASSIE_OPERATIONAL_RESTORE_TARGET")
            .expect("CASSIE_OPERATIONAL_RESTORE_TARGET");

        // Act
        let created = Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions::default(),
        )
        .expect("create operational snapshot");
        let restored_manifest =
            Cassie::restore_snapshot(&snapshot, &restored).expect("restore operational snapshot");

        // Assert
        assert_eq!(restored_manifest, created);
        assert!(std::path::Path::new(&restored).is_dir());
    }

    #[test]
    fn should_reject_v1_snapshot_manifest_with_expected_v2_before_restore() {
        // Arrange
        use_local_storage();
        let source = data_dir("snapshot_incompatible_source");
        let snapshot = data_dir("snapshot_incompatible_bundle");
        let restored = data_dir("snapshot_incompatible_restored");
        seed_replayed_projection(&source, "snapshot_incompatible_docs");
        Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(3_579),
            },
        )
        .unwrap();
        let manifest_path = std::path::Path::new(&snapshot).join("cassie-snapshot-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["format_version"] = serde_json::json!(1);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // Act
        let error = Cassie::restore_snapshot(&snapshot, &restored).unwrap_err();

        // Assert
        assert!(error
            .to_string()
            .contains("snapshot manifest version 1 is unsupported; expected 2"));
        assert!(!std::path::Path::new(&restored).exists());

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
    }

    #[test]
    fn should_reject_restore_when_manifest_epoch_does_not_match_copied_state() {
        // Arrange
        use_local_storage();
        let source = data_dir("snapshot_epoch_mismatch_source");
        let snapshot = data_dir("snapshot_epoch_mismatch_bundle");
        let restored = data_dir("snapshot_epoch_mismatch_restored");
        seed_replayed_projection(&source, "snapshot_epoch_mismatch_docs");
        Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(4_691),
            },
        )
        .expect("create snapshot");
        let manifest_path = std::path::Path::new(&snapshot).join("cassie-snapshot-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["data_epoch"] = serde_json::json!(manifest["data_epoch"].as_u64().unwrap() + 1);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // Act
        let error = Cassie::restore_snapshot(&snapshot, &restored).unwrap_err();

        // Assert
        assert!(error
            .to_string()
            .contains("snapshot data epoch does not match restored data"));
        assert!(!std::path::Path::new(&restored).exists());

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
        let _ = std::fs::remove_dir_all(restored);
    }

    #[cfg(unix)]
    #[test]
    fn should_remove_partial_snapshot_after_copy_error() {
        // Arrange
        use_local_storage();
        let source = data_dir("snapshot_copy_error_source");
        let snapshot = data_dir("snapshot_copy_error_bundle");
        seed_replayed_projection(&source, "snapshot_copy_error_docs");
        let regular_file = std::path::Path::new(&source).join("aaa-copy-marker");
        std::fs::write(&regular_file, b"copy before failure").unwrap();
        let special_file = std::path::Path::new(&source).join("zzz-copy-link");
        std::os::unix::fs::symlink("missing-target", &special_file).unwrap();

        // Act
        let error = Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(5_791),
            },
        )
        .unwrap_err();

        // Assert
        assert!(error
            .to_string()
            .contains("snapshot copy does not support special file"));
        assert!(!std::path::Path::new(&snapshot).exists());

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
    }

    #[cfg(unix)]
    #[test]
    fn should_remove_partial_restore_after_copy_error() {
        // Arrange
        use_local_storage();
        let source = data_dir("restore_copy_error_source");
        let snapshot = data_dir("restore_copy_error_bundle");
        let target = data_dir("restore_copy_error_target");
        seed_replayed_projection(&source, "restore_copy_error_docs");
        Cassie::create_snapshot_from_data_dir(
            &source,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(6_802),
            },
        )
        .unwrap();
        let snapshot_midge = std::path::Path::new(&snapshot).join("midge");
        std::fs::write(
            snapshot_midge.join("aaa-copy-marker"),
            b"copy before failure",
        )
        .unwrap();
        std::os::unix::fs::symlink("missing-target", snapshot_midge.join("zzz-copy-link")).unwrap();

        // Act
        let error = Cassie::restore_snapshot(&snapshot, &target).unwrap_err();

        // Assert
        assert!(error
            .to_string()
            .contains("snapshot copy does not support special file"));
        assert!(!std::path::Path::new(&target).exists());

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(snapshot);
        let _ = std::fs::remove_dir_all(target);
    }

    #[cfg(unix)]
    #[test]
    fn should_reject_snapshot_destination_through_a_symlink_into_the_source() {
        // Arrange
        use_local_storage();
        let source = data_dir("snapshot_symlink_source");
        let alias = data_dir("snapshot_symlink_alias");
        seed_replayed_projection(&source, "snapshot_symlink_docs");
        std::os::unix::fs::symlink(&source, &alias).expect("source alias");

        // Act
        let error = Cassie::create_snapshot_from_data_dir(
            &source,
            std::path::Path::new(&alias).join("nested"),
            CassieSnapshotOptions::default(),
        )
        .expect_err("overlapping destination");

        // Assert
        assert!(error.to_string().contains("must not overlap"));
        let _ = std::fs::remove_file(alias);
        let _ = std::fs::remove_dir_all(source);
    }

    #[test]
    fn should_reject_restore_target_that_contains_the_snapshot() {
        // Arrange
        use_local_storage();
        let source = data_dir("restore_overlap_source");
        let target = data_dir("restore_overlap_target");
        let snapshot = std::path::Path::new(&target).join("bundle");
        std::fs::create_dir_all(&target).expect("target container");
        seed_replayed_projection(&source, "restore_overlap_docs");
        Cassie::create_snapshot_from_data_dir(&source, &snapshot, CassieSnapshotOptions::default())
            .expect("snapshot");

        // Act
        let error = Cassie::restore_snapshot(&snapshot, &target).expect_err("overlapping target");

        // Assert
        assert!(error.to_string().contains("must not overlap"));
        assert!(snapshot.exists(), "restore must not delete its own source");
        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(target);
    }
}

// Formerly tests/storage_integrity.rs.
mod storage_integrity {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::midge::adapter::{
        document_write_storage_failures_remaining, set_document_write_conflicts_remaining,
        set_document_write_storage_failures, DocumentWriteStorageFailure,
    };
    use cassie::types::{DataType, FieldSchema, Schema};

    use super::support_sql as support;

    fn collection_name(collection: &str) -> String {
        canonical_relation_name("postgres", "public", collection)
    }

    fn register_collection(cassie: &Cassie, collection: &str) -> String {
        let collection = collection_name(collection);
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: false,
            }],
        };
        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .expect("create collection");
        cassie.register_collection(
            &collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        collection
    }

    #[test]
    fn should_increment_the_durable_data_epoch_once_per_changed_batch() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("durable_data_epoch");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = register_collection(&cassie, "durable_data_epoch");
        assert_eq!(cassie.midge.data_epoch().expect("read epoch"), 0);

        // Act
        cassie
            .midge
            .put_document(
                &collection,
                Some("first".to_string()),
                serde_json::json!({"title": "first"}),
            )
            .expect("write first row");
        let after_put = cassie.midge.data_epoch().expect("read epoch after put");
        let deleted = cassie
            .midge
            .delete_document(&collection, "missing")
            .expect("delete missing row");
        let after_missing_delete = cassie.midge.data_epoch().expect("read epoch after no-op");
        cassie
            .midge
            .delete_document(&collection, "first")
            .expect("delete row");
        let after_delete = cassie.midge.data_epoch().expect("read epoch after delete");

        // Assert
        assert!(!deleted);
        assert_eq!(after_put, 1);
        assert_eq!(after_missing_delete, after_put);
        assert_eq!(after_delete, 2);
    }

    #[test]
    fn should_hydrate_the_durable_data_epoch_after_restart() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("durable_data_epoch_restart");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = register_collection(&cassie, "durable_data_epoch_restart");
        cassie
            .midge
            .put_document(
                &collection,
                Some("first".to_string()),
                serde_json::json!({"title": "first"}),
            )
            .expect("write row");
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("restart Cassie");

        // Assert
        assert_eq!(restarted.midge.data_epoch().expect("read durable epoch"), 1);
    }

    #[test]
    fn should_increment_data_epoch_for_concurrent_writes_to_different_collections() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("concurrent_data_epoch");
        let cassie = std::sync::Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
        cassie.startup().expect("start Cassie");
        let first = register_collection(&cassie, "concurrent_data_epoch_first");
        let second = register_collection(&cassie, "concurrent_data_epoch_second");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let workers = [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, collection)| {
                let cassie = std::sync::Arc::clone(&cassie);
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    cassie.midge.put_document(
                        &collection,
                        Some(format!("row-{index}")),
                        serde_json::json!({"title": format!("row-{index}")}),
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();

        // Act
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker completed"))
            .collect::<Result<Vec<_>, _>>();

        // Assert
        assert!(results.is_ok());
        assert_eq!(cassie.midge.data_epoch().expect("read data epoch"), 2);
    }

    #[test]
    fn should_leave_data_unchanged_when_write_conflict_retries_are_exhausted() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("write_conflict_retry_exhaustion");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = register_collection(&cassie, "write_conflict_retry_exhaustion");
        set_document_write_conflicts_remaining(8);

        // Act
        let result = cassie.midge.put_document(
            &collection,
            Some("row-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        );
        set_document_write_conflicts_remaining(0);

        // Assert
        assert!(matches!(
            result,
            Err(cassie::app::CassieError::StorageRetryable(_))
        ));
        assert!(cassie
            .midge
            .get_document(&collection, "row-1")
            .expect("read row")
            .is_none());
        assert_eq!(cassie.midge.data_epoch().expect("read data epoch"), 0);
    }

    #[test]
    fn should_retry_a_document_write_batch_after_a_transient_write_stall() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("write_stall_retry");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = register_collection(&cassie, "write_stall_retry");
        set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 2);

        // Act
        let result = cassie.midge.put_document(
            &collection,
            Some("row-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        );
        let unconsumed_failures = document_write_storage_failures_remaining();
        set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 0);

        // Assert
        assert!(result.is_ok(), "write stall was not retried: {result:?}");
        assert_eq!(
            unconsumed_failures, 0,
            "injected write stalls were not all consumed"
        );
        assert!(cassie
            .midge
            .get_document(&collection, "row-1")
            .expect("read row")
            .is_some());

        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_not_retry_a_document_write_batch_after_the_writer_is_fenced() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("fenced_write_no_retry");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = register_collection(&cassie, "fenced_write_no_retry");
        set_document_write_storage_failures(DocumentWriteStorageFailure::Fenced, 1);

        // Act
        let result = cassie.midge.put_document(
            &collection,
            Some("row-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        );
        set_document_write_storage_failures(DocumentWriteStorageFailure::Fenced, 0);

        // Assert
        assert!(
            matches!(
                &result,
                Err(cassie::app::CassieError::StorageRetryable(message)) if message.contains("fenced")
            ),
            "fenced write should fail without an in-process retry: {result:?}"
        );
        assert!(cassie
            .midge
            .get_document(&collection, "row-1")
            .expect("read row")
            .is_none());
        assert_eq!(cassie.midge.data_epoch().expect("read data epoch"), 0);

        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_persist_collection_generation_for_changed_writes_only() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("collection_generation");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = register_collection(&cassie, "collection_generation");
        assert_eq!(
            cassie
                .midge
                .collection_generation(&collection)
                .expect("read generation"),
            0
        );

        // Act
        cassie
            .midge
            .put_document(
                &collection,
                Some("row-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("write row");
        let after_write = cassie
            .midge
            .collection_generation(&collection)
            .expect("read generation after write");
        cassie
            .midge
            .delete_document(&collection, "missing")
            .expect("delete missing row");
        let after_no_op = cassie
            .midge
            .collection_generation(&collection)
            .expect("read generation after no-op");
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("restart Cassie");

        // Assert
        assert_eq!(after_write, 1);
        assert_eq!(after_no_op, after_write);
        assert_eq!(
            restarted
                .midge
                .collection_generation(&collection)
                .expect("read durable generation"),
            after_write
        );
    }
}

// Formerly tests/scalar_index_redundant_bounds.rs.
mod scalar_index_redundant_bounds {
    use super::support_sql as support;

    use cassie::app::{Cassie, CassieSession};
    use cassie::types::Value;

    use support::*;

    fn seed_score_table(cassie: &Cassie, session: &CassieSession, table: &str, indexed: bool) {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {table} (score BIGINT, label TEXT)"),
                vec![],
            )
            .unwrap();
        for score in [4, 5, 6, 9, 10, 11, 12] {
            cassie
                .midge
                .put_document(
                    table,
                    Some(format!("row-{score}")),
                    serde_json::json!({"score": score, "label": format!("label-{score}")}),
                )
                .unwrap();
        }
        if indexed {
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE INDEX {table}_score_idx ON {table} USING btree (score)"),
                    vec![],
                )
                .unwrap();
        }
    }

    fn seed_expression_table(cassie: &Cassie, session: &CassieSession, table: &str, indexed: bool) {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {table} (title TEXT, label TEXT)"),
                vec![],
            )
            .unwrap();
        for title in ["alpha", "beta", "delta", "gamma", "omega"] {
            cassie
                .midge
                .put_document(
                    table,
                    Some(format!("row-{title}")),
                    serde_json::json!({"title": title, "label": format!("label-{title}")}),
                )
                .unwrap();
        }
        if indexed {
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "CREATE INDEX {table}_lower_idx ON {table} USING btree (lower(title))"
                    ),
                    vec![],
                )
                .unwrap();
        }
    }

    fn query_rows(
        cassie: &Cassie,
        session: &CassieSession,
        table: &str,
        projection: &str,
        predicate: &str,
    ) -> Vec<Vec<cassie::types::Value>> {
        cassie
            .execute_sql(
                session,
                &format!(
                    "SELECT {projection} FROM {table} WHERE {predicate} ORDER BY {projection}"
                ),
                vec![],
            )
            .unwrap()
            .rows
    }

    fn query_expression_rows(
        cassie: &Cassie,
        session: &CassieSession,
        table: &str,
        predicate: &str,
    ) -> Vec<Vec<Value>> {
        cassie
            .execute_sql(
                session,
                &format!("SELECT title FROM {table} WHERE {predicate} ORDER BY lower(title)"),
                vec![],
            )
            .unwrap()
            .rows
    }

    #[test]
    fn should_match_full_scan_when_intersecting_repeated_column_bounds() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_index_repeated_column_bounds");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_score_table(&cassie, &session, "repeated_column_indexed", true);
            seed_score_table(&cassie, &session, "repeated_column_baseline", false);

            // Act
            let comparisons = [
                "score > 10 AND score > 5",
                "score >= 10 AND score >= 5",
                "score < 5 AND score < 10",
                "score <= 5 AND score <= 10",
                "score > 10 AND score >= 10",
                "score < 10 AND score <= 10",
            ]
            .map(|predicate| {
                (
                    predicate,
                    query_rows(
                        &cassie,
                        &session,
                        "repeated_column_indexed",
                        "score",
                        predicate,
                    ),
                    query_rows(
                        &cassie,
                        &session,
                        "repeated_column_baseline",
                        "score",
                        predicate,
                    ),
                )
            });
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT score FROM repeated_column_indexed WHERE score > 10 AND score > 5 ORDER BY score",
                    vec![],
                )
                .unwrap();

            // Assert
            for (predicate, indexed, baseline) in comparisons {
                assert_eq!(indexed, baseline, "indexed result diverged for {predicate}");
            }
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("repeated_column_indexed_score_idx"));
            assert!(plan.contains("access_path_reason=scalar-index-range"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_match_full_scan_when_intersecting_repeated_expression_bounds() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_index_repeated_expression_bounds");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_expression_table(&cassie, &session, "repeated_expression_indexed", true);
            seed_expression_table(&cassie, &session, "repeated_expression_baseline", false);

            // Act
            let indexed_lower = query_expression_rows(
                &cassie,
                &session,
                "repeated_expression_indexed",
                "lower(title) > 'delta' AND lower(title) > 'alpha'",
            );
            let baseline_lower = query_expression_rows(
                &cassie,
                &session,
                "repeated_expression_baseline",
                "lower(title) > 'delta' AND lower(title) > 'alpha'",
            );
            let indexed_upper = query_expression_rows(
                &cassie,
                &session,
                "repeated_expression_indexed",
                "lower(title) < 'gamma' AND lower(title) <= 'omega'",
            );
            let baseline_upper = query_expression_rows(
                &cassie,
                &session,
                "repeated_expression_baseline",
                "lower(title) < 'gamma' AND lower(title) <= 'omega'",
            );
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM repeated_expression_indexed WHERE lower(title) > 'delta' AND lower(title) > 'alpha' ORDER BY lower(title)",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(indexed_lower, baseline_lower);
            assert_eq!(indexed_upper, baseline_upper);
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("repeated_expression_indexed_lower_idx"));
            assert!(plan.contains("access_path_reason=scalar-index-range"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_match_full_scan_for_contradictory_column_constraints() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_index_contradictory_column_constraints");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_score_table(&cassie, &session, "contradictory_column_indexed", true);
            seed_score_table(&cassie, &session, "contradictory_column_baseline", false);

            // Act
            let comparisons = [
                "score = 5 AND score = 6",
                "score = 5 AND score > 10",
                "10 < score AND 5 = score",
                "score = 5 AND score >= 5",
                "score = 5 AND 5 = score",
            ]
            .map(|predicate| {
                (
                    predicate,
                    query_rows(
                        &cassie,
                        &session,
                        "contradictory_column_indexed",
                        "score",
                        predicate,
                    ),
                    query_rows(
                        &cassie,
                        &session,
                        "contradictory_column_baseline",
                        "score",
                        predicate,
                    ),
                )
            });
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT score FROM contradictory_column_indexed WHERE score = 5 AND score = 6",
                    vec![],
                )
                .unwrap();

            // Assert
            for (predicate, indexed, baseline) in comparisons {
                assert_eq!(indexed, baseline, "indexed result diverged for {predicate}");
            }
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("contradictory_column_indexed_score_idx"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_match_full_scan_for_contradictory_expression_constraints() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_index_contradictory_expression_constraints");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_expression_table(&cassie, &session, "contradictory_expression_indexed", true);
            seed_expression_table(&cassie, &session, "contradictory_expression_baseline", false);

            // Act
            let comparisons = [
                "lower(title) = 'beta' AND lower(title) = 'delta'",
                "lower(title) = 'beta' AND lower(title) > 'gamma'",
                "'gamma' < lower(title) AND 'beta' = lower(title)",
                "lower(title) = 'beta' AND lower(title) >= 'beta'",
            ]
            .map(|predicate| {
                (
                    predicate,
                    query_expression_rows(
                        &cassie,
                        &session,
                        "contradictory_expression_indexed",
                        predicate,
                    ),
                    query_expression_rows(
                        &cassie,
                        &session,
                        "contradictory_expression_baseline",
                        predicate,
                    ),
                )
            });
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM contradictory_expression_indexed WHERE lower(title) = 'beta' AND lower(title) = 'delta'",
                    vec![],
                )
                .unwrap();
            let indexed_projection = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM contradictory_expression_indexed WHERE lower(title) = 'beta'",
                    vec![],
                )
                .unwrap();
            let baseline_projection = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM contradictory_expression_baseline WHERE lower(title) = 'beta'",
                    vec![],
                )
                .unwrap();

            // Assert
            for (predicate, indexed, baseline) in comparisons {
                assert_eq!(indexed, baseline, "indexed result diverged for {predicate}");
            }
            assert_eq!(indexed_projection.rows, baseline_projection.rows);
            assert_eq!(
                indexed_projection.rows,
                vec![vec![Value::String("label-beta".to_string())]]
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("contradictory_expression_indexed_lower_idx"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_apply_residual_predicates_before_a_scalar_index_limit() {
        // Arrange
        use_local_storage();
        let path = data_dir("scalar_index_residual_predicate_limit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_score_table(&cassie, &session, "residual_limit_indexed", false);
            seed_score_table(&cassie, &session, "residual_limit_baseline", false);
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX residual_limit_idx ON residual_limit_indexed USING btree (score, label)",
                    vec![],
                )
                .unwrap();

            // Act
            let indexed = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM residual_limit_indexed WHERE score > 4 AND label = 'label-11' ORDER BY score LIMIT 1",
                    vec![],
                )
                .unwrap();
            let baseline = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM residual_limit_baseline WHERE score > 4 AND label = 'label-11' ORDER BY score LIMIT 1",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT label FROM residual_limit_indexed WHERE score > 4 AND label = 'label-11' ORDER BY score LIMIT 1",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(indexed.rows, baseline.rows);
            assert_eq!(indexed.rows, vec![vec![Value::String("label-11".to_string())]]);
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("residual_limit_idx"));
            assert!(plan.contains("access_path_reason=scalar-index-range"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
