// Consolidated integration suite: search.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/fulltext_index_completeness.rs.
mod fulltext_index_completeness {
    use cassie::app::Cassie;
    use cassie::catalog::{canonical_relation_name, DEFAULT_SCHEMA};
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::Value;
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_executor as support;
    use support::{
        create_text_collection, data_dir, fulltext_index, put_document, put_fulltext_index,
        use_local_storage,
    };

    const COLLECTION: &str = "fulltext_index_completeness";
    const INDEX: &str = "fulltext_body_idx";

    fn seed_index(path: &str, documents: impl IntoIterator<Item = (String, String)>) -> Cassie {
        let cassie = Cassie::new_with_data_dir(path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = canonical_relation_name("postgres", DEFAULT_SCHEMA, COLLECTION);
        create_text_collection(&cassie, &collection, &["id", "body"]);
        for (id, body) in documents {
            put_document(&cassie, &collection, &id, serde_json::json!({"body": body}));
        }
        put_fulltext_index(&cassie, &collection, INDEX, "body", &[]);
        cassie
            .catalog
            .register_index(fulltext_index(&collection, INDEX, "body", &[]));
        cassie
    }

    fn artifacts_with_magic(cassie: &Cassie, magic: [u8; 4]) -> Vec<(Vec<u8>, Vec<u8>)> {
        let prefix = cassie
            .midge
            .fulltext_artifact_prefix_for_diagnostics(COLLECTION, INDEX)
            .expect("fulltext artifact prefix");
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("fulltext artifacts")
            .into_iter()
            .filter(|(_, value)| value.starts_with(&magic))
            .collect()
    }

    fn delete_data_key(cassie: &Cassie, key: Vec<u8>) {
        let mut tx = cassie
            .midge
            .data_tx(TransactionMode::ReadWrite)
            .expect("data transaction");
        tx.delete(key).expect("delete data key");
        tx.commit(WriteOptions::sync()).expect("commit deletion");
    }

    fn replace_artifact(cassie: &Cassie, magic: [u8; 4], replacement: Vec<u8>) {
        let (key, _) = artifacts_with_magic(cassie, magic)
            .into_iter()
            .next()
            .expect("artifact record");
        let mut tx = cassie
            .midge
            .data_tx(TransactionMode::ReadWrite)
            .expect("data transaction");
        tx.put(key, replacement, None).expect("replace artifact");
        tx.commit(WriteOptions::sync())
            .expect("commit artifact replacement");
    }

    fn search(cassie: &Cassie, sql: &str) -> cassie::executor::QueryResult {
        cassie
            .execute_sql(&cassie.create_session("tester", None), sql, vec![])
            .expect("fulltext query")
    }

    fn fallback_count(cassie: &Cassie, reason: &str) -> u64 {
        cassie.metrics()["search"]["retrieval_fallback_reasons"][reason]
            .as_u64()
            .unwrap_or_default()
    }

    fn long_document_id(index: usize) -> String {
        format!("{index:03}-{}", format!("{index:03}").repeat(300))
    }

    #[test]
    fn should_fallback_when_one_posting_block_is_missing_among_valid_blocks() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_missing_posting_block");
        let documents = (0..96).map(|index| (long_document_id(index), "alpha".to_string()));
        let cassie = seed_index(&path, documents);
        let blocks = artifacts_with_magic(&cassie, *b"FTB1");
        assert!(blocks.len() > 1, "fixture must span posting blocks");
        delete_data_key(&cassie, blocks[1].0.clone());
        let before = fallback_count(&cassie, "invalid_persisted_artifact");

        // Act
        let result = search(
        &cassie,
        "SELECT id, search_score(body, 'alpha') AS score FROM fulltext_index_completeness WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 100",
    );

        // Assert
        assert_eq!(result.rows.len(), 96);
        assert!(fallback_count(&cassie, "invalid_persisted_artifact") > before);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_when_a_candidate_is_missing_document_statistics() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_missing_document_stats");
        let cassie = seed_index(
            &path,
            [
                ("d1".to_string(), "alpha".to_string()),
                ("d2".to_string(), "alpha".to_string()),
            ],
        );
        let stats = artifacts_with_magic(&cassie, *b"FTD1");
        delete_data_key(&cassie, stats[0].0.clone());
        let before = fallback_count(&cassie, "invalid_persisted_artifact");

        // Act
        let result = search(
        &cassie,
        "SELECT id, search_score(body, 'alpha') AS score FROM fulltext_index_completeness WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 10",
    );

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert!(fallback_count(&cassie, "invalid_persisted_artifact") > before);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_before_top_k_publishes_a_dangling_candidate() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_dangling_candidate");
        let cassie = seed_index(
            &path,
            [
                ("d1".to_string(), "alpha".to_string()),
                ("d2".to_string(), "alpha".to_string()),
            ],
        );
        let source_key = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("data records")
            .into_iter()
            .find(|(_, value)| value.starts_with(b"CRB2"))
            .map(|(key, _)| key)
            .expect("source row");
        delete_data_key(&cassie, source_key);
        let before = fallback_count(&cassie, "missing_candidate_row");

        // Act
        let result = search(
        &cassie,
        "SELECT id, search_score(body, 'alpha') AS score FROM fulltext_index_completeness WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 10",
    );

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert!(fallback_count(&cassie, "missing_candidate_row") > before);
        assert!(matches!(result.rows[0][0], Value::String(_)));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_a_malformed_manifest_during_startup() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_malformed_manifest_restart");
        let cassie = seed_index(&path, [("d1".to_string(), "alpha beta".to_string())]);
        replace_artifact(&cassie, *b"FTG1", b"malformed-manifest".to_vec());
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("reconcile fulltext sidecar");
        let state = restarted
            .midge
            .get_persisted_fulltext_index_state(COLLECTION, INDEX)
            .expect("read reconciled fulltext state")
            .expect("fulltext state");

        // Assert
        assert_eq!(state.total_documents, 1);
        assert_eq!(state.postings["alpha"].len(), 1);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_old_fulltext_metadata_during_startup() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_old_metadata_restart");
        let cassie = seed_index(&path, [("d1".to_string(), "alpha beta".to_string())]);
        let (_, mut metadata) = artifacts_with_magic(&cassie, *b"FTM1")
            .into_iter()
            .next()
            .expect("fulltext metadata");
        metadata[4] = 1;
        replace_artifact(&cassie, *b"FTM1", metadata);
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("upgrade fulltext sidecar");
        let metadata = artifacts_with_magic(&restarted, *b"FTM1")
            .into_iter()
            .next()
            .map(|(_, value)| value)
            .expect("upgraded fulltext metadata");

        // Assert
        assert_eq!(metadata[4], 2, "startup must publish only v2 metadata");
        assert!(restarted
            .midge
            .get_persisted_fulltext_index_state(COLLECTION, INDEX)
            .expect("read upgraded fulltext state")
            .is_some());
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/fulltext_persisted_retrieval.rs.
mod fulltext_persisted_retrieval {
    use cassie::app::Cassie;
    use std::collections::BTreeSet;

    use super::support_executor as support;
    use support::{
        cassie_temp, create_text_collection, data_dir, put_document, put_fulltext_index,
        use_local_storage,
    };

    #[test]
    fn should_persist_generation_bound_fulltext_state() {
        // Arrange
        let cassie = cassie_temp("fulltext_persisted_retrieval");
        let collection = "fulltext_persisted_retrieval";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha alpha beta"}),
        );
        put_document(
            &cassie,
            collection,
            "d2",
            serde_json::json!({"body": "beta gamma"}),
        );

        // Act
        put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
        let state = cassie
            .midge
            .get_persisted_fulltext_index_state(collection, "fulltext_body_idx")
            .expect("read persisted fulltext state")
            .expect("state after index publication");

        // Assert
        assert_eq!(
            state.built_generation,
            cassie.midge.collection_generation(collection).unwrap()
        );
        assert_eq!(state.total_documents, 2);
        assert_eq!(state.documents_with_text, 2);
        assert_eq!(state.document_stats.get("d1").unwrap().doc_length, 3);
        assert_eq!(state.postings.get("alpha").unwrap()[0].term_frequency, 2);
    }

    #[test]
    fn should_refresh_fulltext_postings_after_mutation() {
        // Arrange
        let cassie = cassie_temp("fulltext_persisted_mutation");
        let collection = "fulltext_persisted_mutation";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha alpha beta"}),
        );
        put_document(
            &cassie,
            collection,
            "d2",
            serde_json::json!({"body": "beta gamma"}),
        );
        put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);

        // Act
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "delta"}),
        );
        cassie.midge.delete_document(collection, "d2").unwrap();
        let state = cassie
            .midge
            .get_persisted_fulltext_index_state(collection, "fulltext_body_idx")
            .unwrap()
            .unwrap();

        // Assert
        assert!(!state.postings.contains_key("alpha"));
        assert_eq!(state.document_stats.get("d1").unwrap().doc_length, 1);
        assert_eq!(state.total_documents, 1);
        assert!(!state.document_stats.contains_key("d2"));
    }

    #[test]
    fn should_reload_persisted_fulltext_state_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_persisted_restart");
        let collection = "fulltext_persisted_restart";
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha beta"}),
        );
        put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let state = restarted
            .midge
            .get_persisted_fulltext_index_state(collection, "fulltext_body_idx")
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(state.total_documents, 1);
        assert_eq!(state.postings["alpha"][0].document_id, "d1");
    }

    #[test]
    fn should_bound_persisted_term_candidate_reads() {
        // Arrange
        let cassie = cassie_temp("fulltext_bounded_candidates");
        let collection = "fulltext_bounded_candidates";
        create_text_collection(&cassie, collection, &["id", "body"]);
        for index in 0..64 {
            let body = if index < 2 {
                "alpha marker"
            } else {
                "unrelated"
            };
            put_document(
                &cassie,
                collection,
                &format!("d{index}"),
                serde_json::json!({"body": body}),
            );
        }
        put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
        let before = cassie.metrics();

        // Act
        let candidates = cassie
            .midge
            .fulltext_candidate_stats(collection, "fulltext_body_idx", &["alpha".to_string()])
            .expect("candidate stats");
        let after = cassie.metrics();

        // Assert
        assert_eq!(candidates.len(), 2);
        assert!(candidates.contains_key("d0"));
        assert!(candidates.contains_key("d1"));
        let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
            - before["storage"]["data"]["reads"].as_u64().unwrap();
        assert!(
            reads < 64,
            "expected bounded persisted reads, observed {reads}"
        );
    }

    #[test]
    fn should_fetch_only_allowed_fulltext_candidate_statistics_given_broad_term() {
        // Arrange
        let cassie = cassie_temp("fulltext_allowed_candidates");
        let collection = "fulltext_allowed_candidates";
        create_text_collection(&cassie, collection, &["id", "body"]);
        for index in 0..64 {
            put_document(
                &cassie,
                collection,
                &format!("d{index}"),
                serde_json::json!({"body": "common marker"}),
            );
        }
        put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
        let allowed = BTreeSet::from(["d7".to_string(), "d41".to_string()]);
        let before = cassie.metrics();

        // Act
        let candidates = cassie
            .midge
            .fulltext_candidate_stats_for_ids(
                collection,
                "fulltext_body_idx",
                &["common".to_string()],
                &allowed,
            )
            .expect("bounded candidate stats");
        let after = cassie.metrics();

        // Assert
        assert_eq!(candidates.keys().cloned().collect::<BTreeSet<_>>(), allowed);
        let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
            - before["storage"]["data"]["reads"].as_u64().unwrap();
        assert!(
            reads < 16,
            "expected allowed-id point reads, observed {reads}"
        );
    }
}

// Formerly tests/fulltext_persisted_sql.rs.
mod fulltext_persisted_sql {
    use super::support_sql as support;

    use cassie::app::{Cassie, CassieError};
    use cassie::config::CassieRuntimeConfig;
    use cassie::runtime::QueryCancellationHandle;
    use cassie::types::Value;
    use std::sync::Arc;
    use std::time::Duration;

    use support::*;

    #[test]
    fn should_score_case_sensitive_terms_from_persisted_fulltext_index() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_case_sensitive_fulltext");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE persisted_case_docs (body TEXT)",
                vec![],
            )
            .expect("create case-sensitive search table");
        for body in ["The Rust compiler is fast", "a rust tool"] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO persisted_case_docs (body) VALUES ($1)",
                    vec![Value::String(body.to_string())],
                )
                .expect("insert case-sensitive search row");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_case_body_idx ON persisted_case_docs USING fulltext (body) WITH (case_folding = 'false')",
                vec![],
            )
            .expect("create case-sensitive fulltext index");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT body, search_score(body, 'Rust') AS score FROM persisted_case_docs WHERE search(body, 'Rust')",
                vec![],
            )
            .expect("query exact-case persisted term");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            result.rows[0][0],
            Value::String("The Rust compiler is fast".to_string())
        );
        assert!(matches!(result.rows[0][1], Value::Float64(score) if score > 0.0));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_read_persisted_postings_before_fetching_candidate_rows() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_sql");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE persisted_search_docs (title TEXT, body TEXT)",
                vec![],
            )
            .expect("create search table");
        for (title, body) in [
            ("first", "alpha beta"),
            ("second", "bravo charlie"),
            ("third", "alpha alpha delta"),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO persisted_search_docs (title, body) VALUES ($1, $2)",
                    vec![
                        Value::String(title.to_string()),
                        Value::String(body.to_string()),
                    ],
                )
                .expect("insert search row");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_search_body_idx ON persisted_search_docs USING fulltext (body)",
                vec![],
            )
            .expect("create fulltext index");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title, snippet(body, $1) AS excerpt, search_score(body, $1) AS score FROM persisted_search_docs WHERE search(body, $1) LIMIT 1",
                vec![Value::String("alpha".to_string())],
            )
            .expect("query persisted postings");
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert!(matches!(result.rows[0][0], Value::String(_)));
        assert!(
            matches!(&result.rows[0][2], Value::String(excerpt) if excerpt.contains("<mark>alpha</mark>"))
        );
        assert!(matches!(result.rows[0][3], Value::Float64(score) if score > 0.0));
        assert!(
            after["search"]["posting_reads_total"].as_u64().unwrap()
                > before["search"]["posting_reads_total"].as_u64().unwrap()
        );
        assert_eq!(
            after["search"]["candidate_row_fetches_total"]
                .as_u64()
                .unwrap()
                - before["search"]["candidate_row_fetches_total"]
                    .as_u64()
                    .unwrap(),
            1
        );
        assert_eq!(
            after["search"]["row_scan_fallback_total"].as_u64(),
            before["search"]["row_scan_fallback_total"].as_u64()
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_match_row_baseline_scores_from_persisted_postings() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_scores");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE persisted_score_docs (body TEXT)",
                vec![],
            )
            .expect("create score table");
        for body in ["alpha beta", "alpha alpha alpha beta", "beta gamma"] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO persisted_score_docs (body) VALUES ($1)",
                    vec![Value::String(body.to_string())],
                )
                .expect("insert score row");
        }
        let sql = "SELECT id, search_score(body, $1) AS score FROM persisted_score_docs WHERE search(body, $1) ORDER BY score DESC LIMIT 2";
        let query_params = || vec![Value::String("alpha beta".to_string())];
        let baseline = cassie
            .execute_sql(&session, sql, query_params())
            .expect("execute row baseline");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_score_body_idx ON persisted_score_docs USING fulltext (body)",
                vec![],
            )
            .expect("create score index");
        let before = cassie.metrics();

        // Act
        let persisted = cassie
            .execute_sql(&session, sql, query_params())
            .expect("execute persisted score query");
        let after = cassie.metrics();

        // Assert
        assert_eq!(persisted.rows, baseline.rows);
        assert!(
            after["search"]["posting_reads_total"].as_u64().unwrap()
                > before["search"]["posting_reads_total"].as_u64().unwrap()
        );
        assert_eq!(
            after["search"]["candidate_row_fetches_total"].as_u64(),
            before["search"]["candidate_row_fetches_total"].as_u64()
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_apply_structured_predicates_only_to_posting_candidates() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_structured_filter");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE persisted_filter_docs (category TEXT, body TEXT)",
                vec![],
            )
            .expect("create filtered table");
        for (category, body) in [
            ("keep", "alpha beta"),
            ("drop", "alpha gamma"),
            ("keep", "bravo delta"),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO persisted_filter_docs (category, body) VALUES ($1, $2)",
                    vec![
                        Value::String(category.to_string()),
                        Value::String(body.to_string()),
                    ],
                )
                .expect("insert filtered row");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_filter_body_idx ON persisted_filter_docs USING fulltext (body)",
                vec![],
            )
            .expect("create filtered index");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_filter_category_idx ON persisted_filter_docs (category)",
                vec![],
            )
            .expect("create category index");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, category, search_score(body, $1) AS score FROM persisted_filter_docs WHERE search(body, $1) AND category = $2",
                vec![
                    Value::String("alpha".to_string()),
                    Value::String("keep".to_string()),
                ],
            )
            .expect("query filtered postings");
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][1], Value::String("keep".to_string()));
        assert_eq!(
            after["search"]["candidate_row_fetches_total"]
                .as_u64()
                .unwrap()
                - before["search"]["candidate_row_fetches_total"]
                    .as_u64()
                    .unwrap(),
            1
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_prefilter_fulltext_candidates_with_integer_parameter_on_float_index() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_float_prefilter");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE persisted_float_filter_docs (rating FLOAT, body TEXT)",
            "CREATE INDEX persisted_float_filter_rating_idx ON persisted_float_filter_docs (rating)",
            "INSERT INTO persisted_float_filter_docs (rating, body) VALUES (5, 'alpha beta')",
            "INSERT INTO persisted_float_filter_docs (rating, body) VALUES (7, 'alpha gamma')",
            "CREATE INDEX persisted_float_filter_body_idx ON persisted_float_filter_docs USING fulltext (body)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT rating, search_score(body, $1) AS score FROM persisted_float_filter_docs WHERE search(body, $1) AND rating = $2",
                vec![Value::String("alpha".to_string()), Value::Int64(5)],
            )
            .expect("query float-filtered postings");
        let after = cassie.metrics();

        // Assert
        let ratings = result
            .rows
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>();
        assert_eq!(ratings, vec![Value::Float64(5.0)]);
        assert_eq!(
            after["search"]["retrieval_fallback_reasons"]["authoritative_row_scan"],
            serde_json::Value::Null
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_null_trailing_index_keys_in_fulltext_prefilter() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_nullable_prefilter");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE persisted_nullable_filter_docs (category TEXT, label TEXT, body TEXT)",
            "INSERT INTO persisted_nullable_filter_docs (category, label, body) VALUES ('keep', 'named', 'alpha beta')",
            "INSERT INTO persisted_nullable_filter_docs (category, label, body) VALUES ('keep', NULL, 'alpha gamma')",
            "CREATE INDEX persisted_nullable_filter_body_idx ON persisted_nullable_filter_docs USING fulltext (body)",
            "CREATE INDEX persisted_nullable_filter_category_idx ON persisted_nullable_filter_docs (category, label)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let mut labels = cassie
            .execute_sql(
                &session,
                "SELECT label, search_score(body, $1) AS score FROM persisted_nullable_filter_docs WHERE search(body, $1) AND category = $2",
                vec![
                    Value::String("alpha".to_string()),
                    Value::String("keep".to_string()),
                ],
            )
            .expect("query nullable-filtered postings")
            .rows
            .into_iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>();
        let after = cassie.metrics();
        labels.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));

        // Assert
        assert_eq!(
            labels,
            vec![Value::Null, Value::String("named".to_string())]
        );
        assert_eq!(
            after["search"]["retrieval_fallback_reasons"]["authoritative_row_scan"],
            serde_json::Value::Null
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_overlay_transaction_mutations_on_fulltext_fallback() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_transaction_overlay");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE persisted_tx_docs (title TEXT, body TEXT)",
                vec![],
            )
            .expect("create transaction table");
        for title in ["keep", "change", "remove"] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO persisted_tx_docs (title, body) VALUES ($1, $2)",
                    vec![
                        Value::String(title.to_string()),
                        Value::String("alpha".to_string()),
                    ],
                )
                .expect("insert transaction row");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_tx_body_idx ON persisted_tx_docs USING fulltext (body)",
                vec![],
            )
            .expect("create transaction index");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(
                &session,
                "UPDATE persisted_tx_docs SET body = $1 WHERE title = $2",
                vec![
                    Value::String("bravo".to_string()),
                    Value::String("change".to_string()),
                ],
            )
            .expect("update transaction row");
        cassie
            .execute_sql(
                &session,
                "DELETE FROM persisted_tx_docs WHERE title = $1",
                vec![Value::String("remove".to_string())],
            )
            .expect("delete transaction row");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO persisted_tx_docs (title, body) VALUES ($1, $2)",
                vec![
                    Value::String("new".to_string()),
                    Value::String("alpha".to_string()),
                ],
            )
            .expect("insert transaction overlay");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title, search_score(body, $1) AS score FROM persisted_tx_docs WHERE search(body, $1)",
                vec![Value::String("alpha".to_string())],
            )
            .expect("query transaction overlay");
        let after = cassie.metrics();

        // Assert
        let mut titles = result
            .rows
            .iter()
            .filter_map(|row| row.get(1).and_then(Value::as_str))
            .collect::<Vec<_>>();
        titles.sort_unstable();
        assert_eq!(titles, vec!["keep", "new"]);
        assert!(
            after["search"]["row_scan_fallback_total"].as_u64().unwrap()
                > before["search"]["row_scan_fallback_total"]
                    .as_u64()
                    .unwrap()
        );
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("rollback transaction");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_enforce_memory_budget_during_persisted_fulltext_scoring() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_memory_budget");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = 512;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE persisted_memory_docs (body TEXT)",
                vec![],
            )
            .expect("create memory table");
        for index in 0..20 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO persisted_memory_docs (body) VALUES ($1)",
                    vec![Value::String(format!(
                        "alpha fixture token number {index} with bounded candidate statistics"
                    ))],
                )
                .expect("insert memory row");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_memory_body_idx ON persisted_memory_docs USING fulltext (body)",
                vec![],
            )
            .expect("create memory index");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, $1) AS score FROM persisted_memory_docs WHERE search(body, $1) ORDER BY score DESC LIMIT 5",
                vec![Value::String("alpha".to_string())],
            )
            .expect_err("candidate statistics should exceed memory budget");

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_persisted_fulltext_scoring_at_a_candidate_boundary() {
        // Arrange
        use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = data_dir("persisted_fulltext_cancellation");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE persisted_cancel_docs (body TEXT)",
                vec![],
            )
            .expect("create cancellation table");
        let rows = (0..50_000)
            .map(|index| {
                (
                    Some(format!("cancel-{index:05}")),
                    serde_json::json!({"body": format!("alpha beta candidate {index:05}")}),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents("persisted_cancel_docs", rows)
            .expect("seed cancellation rows");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX persisted_cancel_body_idx ON persisted_cancel_docs USING fulltext (body)",
                vec![],
            )
            .expect("create cancellation index");
        let cancellation = QueryCancellationHandle::new();
        let query_cancellation = cancellation.clone();
        let query_cassie = Arc::clone(&cassie);
        let query = std::thread::spawn(move || {
            let session = query_cassie.create_session("tester", None);
            query_cassie.execute_sql_with_cancellation(
                &session,
                "SELECT id, search_score(body, $1) AS score FROM persisted_cancel_docs WHERE search(body, $1) ORDER BY score DESC LIMIT 20",
                vec![Value::String("alpha beta".to_string())],
                &query_cancellation,
            )
        });
        std::thread::sleep(Duration::from_millis(10));

        // Act
        cancellation.cancel();
        let error = query
            .join()
            .expect("query thread")
            .expect_err("persisted scoring should observe cancellation");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }
}
// Formerly tests/fulltext_publication_recovery.rs.
mod fulltext_publication_recovery {
    use cassie::app::Cassie;
    use cassie::midge::adapter::set_fulltext_maintenance_failure_point;

    use super::support_executor as support;
    use support::{
        cassie_temp, create_text_collection, data_dir, put_document, put_fulltext_index,
        use_local_storage,
    };

    #[test]
    fn should_replay_fulltext_publication_debt_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_publication_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = "fulltext_publication_recovery";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha"}),
        );
        put_fulltext_index(&cassie, collection, "body_idx", "body", &[]);

        // Act
        set_fulltext_maintenance_failure_point(true);
        put_document(
            &cassie,
            collection,
            "d2",
            serde_json::json!({"body": "beta"}),
        );
        assert!(cassie
            .midge
            .has_fulltext_maintenance_debt(collection, "body_idx")
            .expect("read fulltext debt"));
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay fulltext debt");

        // Assert
        assert!(!restarted
            .midge
            .has_fulltext_maintenance_debt(collection, "body_idx")
            .expect("read recovered fulltext debt"));
        let state = restarted
            .midge
            .get_persisted_fulltext_index_state(collection, "body_idx")
            .expect("read rebuilt fulltext state")
            .expect("state exists");
        assert!(state.postings.contains_key("beta"));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_report_fulltext_retrieval_stage_metrics() {
        // Arrange
        let cassie = cassie_temp("fulltext_retrieval_metrics");
        let collection = "fulltext_retrieval_metrics";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha"}),
        );
        put_fulltext_index(&cassie, collection, "body_idx", "body", &[]);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        cassie
            .execute_sql(
                &session,
                "SELECT id FROM fulltext_retrieval_metrics WHERE search(body, 'alpha')",
                vec![],
            )
            .expect("search");
        let after = cassie.metrics();

        // Assert
        assert!(
            after["search"]["retrieval_stage_queries_total"]
                .as_u64()
                .unwrap_or_default()
                > before["search"]["retrieval_stage_queries_total"]
                    .as_u64()
                    .unwrap_or_default()
        );
        assert!(
            after["search"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap_or_default()
                > before["search"]["row_scan_fallback_total"]
                    .as_u64()
                    .unwrap_or_default()
        );
    }
}

// Formerly tests/fulltext_retrieval_corruption.rs.
mod fulltext_retrieval_corruption {
    use cassie::app::Cassie;
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::Value;
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_executor as support;
    use support::{
        cassie_temp, create_text_collection, fulltext_index, put_document, put_fulltext_index,
    };

    fn seed_corruptible_index() -> Cassie {
        let cassie = cassie_temp("fulltext_retrieval_corruption");
        let collection = "fulltext_retrieval_corruption";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha beta"}),
        );
        put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
        cassie
            .catalog
            .register_index(fulltext_index(collection, "fulltext_body_idx", "body", &[]));
        cassie
    }

    fn corrupt_one_posting(cassie: &Cassie) {
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap();
        let (key, _) = entries
            .into_iter()
            .find(|(_, value)| value.starts_with(b"FTB1"))
            .expect("persisted posting");
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(key, b"corrupt-posting".to_vec(), None).unwrap();
        tx.commit(WriteOptions::sync()).unwrap();
    }

    #[test]
    fn should_reject_corrupt_persisted_fulltext_postings() {
        // Arrange
        let cassie = seed_corruptible_index();
        corrupt_one_posting(&cassie);

        // Act
        let error = cassie
            .midge
            .get_persisted_fulltext_index_state(
                "fulltext_retrieval_corruption",
                "fulltext_body_idx",
            )
            .expect_err("corrupt posting must be rejected");

        // Assert
        assert!(error.to_string().contains("invalid fulltext posting"));
    }

    #[test]
    fn should_fallback_to_rows_when_persisted_fulltext_postings_are_corrupt() {
        // Arrange
        let cassie = seed_corruptible_index();
        corrupt_one_posting(&cassie);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT id, search_score(body, $1) AS score FROM fulltext_retrieval_corruption WHERE search(body, $1)",
            vec![Value::String("alpha".to_string())],
        )
        .expect("query must use deterministic row fallback");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            result.rows[0][0],
            cassie::types::Value::String("d1".to_string())
        );
        let after = cassie.metrics();
        assert!(
            after["search"]["row_scan_fallback_total"].as_u64().unwrap()
                > before["search"]["row_scan_fallback_total"]
                    .as_u64()
                    .unwrap()
        );
        assert!(
            after["search"]["retrieval_fallback_reasons"]["invalid_persisted_artifact"]
                .as_u64()
                .is_some_and(|count| count > 0)
        );
    }
}

// Formerly tests/integration_sql_fulltext_query.rs.
mod integration_sql_fulltext_query {
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
    fn should_order_fulltext_top_k_by_score_with_limit() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_top_k_limit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_top_k_limit";
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
                serde_json::json!({"body": "alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha alpha alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "beta gamma"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_top_k_limit WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_unordered_fulltext_query_with_matching_search_predicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_unordered_match");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_match";
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
                serde_json::json!({"title": "first", "body": "alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "second", "body": "bravo"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "third", "body": "alpha alpha"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_match WHERE search(body, 'alpha') LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));
        assert_eq!(result.rows[0][1], Value::String("third".to_string()));
        assert!(matches!(result.rows[0][2], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_project_search_function_as_boolean_match() {
        // Arrange
        use_local_storage();
        let path = data_dir("search_boolean_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_search_boolean_projection";
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
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha beta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search(body, 'alpha') AS matches_alpha, search(body, 'gamma') AS matches_gamma FROM sql_search_boolean_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Bool(true), Value::Bool(false)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_project_search_score_as_numeric_relevance() {
        // Arrange
        use_local_storage();
        let path = data_dir("search_score_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_search_score_projection";
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
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha alpha beta"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "gamma delta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_search_score_projection ORDER BY id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(score) if score > 0.0));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[1][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fall_back_for_unordered_fulltext_mismatched_search_query_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_unordered_mismatch");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_mismatch";
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
                serde_json::json!({"body": "bravo"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_mismatch WHERE search(body, 'bravo')",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fall_back_for_unordered_fulltext_additional_filters_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_unordered_extra_filter");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_extra_filter";
        let schema = Schema {
            fields: vec![
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
                serde_json::json!({"body": "alpha alpha", "status": "pending"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha", "status": "approved"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_extra_filter WHERE search(body, 'alpha') AND status = 'approved'",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fall_back_for_complex_fulltext_query_without_changing_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_complex_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_complex_fallback";
        let schema = Schema {
            fields: vec![
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
                serde_json::json!({"body": "alpha alpha alpha", "status": "pending"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha", "status": "approved"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_complex_fallback WHERE search(body, 'alpha') AND status = 'approved' ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_project_snippet_without_highlighting_generated_markup() {
        // Arrange
        use_local_storage();
        let path = data_dir("snippet_generated_markup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "sql_snippet_generated_markup";
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
                    serde_json::json!({"body": "alpha beta"}),
                )
                .unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
            .execute_sql(
                &session,
                "SELECT snippet(body, 'alpha mark') AS excerpt FROM sql_snippet_generated_markup",
                vec![],
            )
            .unwrap();

            // Assert
            assert_eq!(
                result.rows,
                vec![vec![Value::String("<mark>alpha</mark> beta".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_fulltext_analyzer_options_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_analyzer_restart");
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
                    "CREATE TABLE sql_fulltext_analyzer_restart (id TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_fulltext_analyzer_restart (id, body) VALUES ('d1', 'the alpha marker')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_sql_fulltext_analyzer_restart ON sql_fulltext_analyzer_restart USING fulltext (body) WITH (analyzer = standard, stop_words = none)",
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
                "sql_fulltext_analyzer_restart",
                "idx_sql_fulltext_analyzer_restart",
            )
            .expect("index should hydrate");
        let session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &session,
                "SELECT search(body, 'the') AS matched, snippet(body, 'the') AS excerpt FROM sql_fulltext_analyzer_restart",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(
            index.options.get("stop_words"),
            Some(&"none".to_string())
        );
        assert_eq!(result.rows[0][0], Value::Bool(true));
        let Value::String(excerpt) = &result.rows[0][1] else {
            panic!("expected snippet string");
        };
        assert!(excerpt.contains("<mark>the</mark>"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_hydrate_fulltext_tokenizer_options_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("fulltext_tokenizer_restart");
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
                    "CREATE TABLE sql_fulltext_tokenizer_restart (body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_fulltext_tokenizer_restart (body) VALUES ('alpha-beta gamma')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_sql_fulltext_tokenizer_restart ON sql_fulltext_tokenizer_restart USING fulltext (body) WITH (tokenizer = whitespace, stop_words = none)",
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
                "sql_fulltext_tokenizer_restart",
                "idx_sql_fulltext_tokenizer_restart",
            )
            .expect("index should hydrate");
        let session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &session,
                "SELECT search(body, 'alpha') AS standard_match, search(body, 'alpha-beta') AS whitespace_match, snippet(body, 'alpha-beta') AS excerpt FROM sql_fulltext_tokenizer_restart",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(
            index.options.get("tokenizer"),
            Some(&"whitespace".to_string())
        );
        assert_eq!(result.rows[0][0], Value::Bool(false));
        assert_eq!(result.rows[0][1], Value::Bool(true));
        let Value::String(excerpt) = &result.rows[0][2] else {
            panic!("expected snippet string");
        };
        assert!(excerpt.contains("<mark>alpha-beta</mark>"));

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/search_vector.rs.
mod search_vector {
    use cassie::hybrid::hybrid_score;
    use cassie::search::bm25;
    use cassie::search::tokenizer;
    use cassie::vector::{cosine_distance, dot_distance, dot_score, l2_distance};

    fn assert_f64_close(actual: f64, expected: f64) {
        let tolerance = f64::EPSILON.max(expected.abs() * 1e-12);
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {actual} to equal {expected}"
        );
    }

    fn usize_to_f64(value: usize) -> f64 {
        value
            .to_string()
            .parse::<f64>()
            .expect("test count should fit f64")
    }

    #[test]
    fn should_tokenize_text_into_lowercase_terms() {
        // Arrange
        let input = "Hello, THE world and the universe";

        // Act
        let tokens = tokenizer::tokenize(input);

        // Assert
        assert_eq!(tokens, vec!["hello", "world", "universe"]);
    }

    #[test]
    fn should_compute_vector_distances_deterministically() {
        // Arrange
        let a = vec![1.0f32, 2.0, 3.0];
        let b = vec![1.0f32, 2.0, 3.0];
        let c = vec![4.0f32, 5.0, 6.0];

        // Act
        let same_distance = l2_distance(&a, &b);
        let different_distance = l2_distance(&a, &c);
        let cosine = cosine_distance(&a, &a);
        let dot_distance_score = dot_distance(&a, &a);
        let dot = dot_score(&a, &b);

        // Assert
        assert_f64_close(same_distance, 0.0);
        assert_f64_close(different_distance, 5.196_152_422_706_632);
        assert_f64_close(cosine, 0.0);
        assert_f64_close(dot_distance_score, -14.0);
        assert_f64_close(dot, 14.0);
    }

    #[test]
    fn should_compute_vector_distances_with_simd_tail_elements() {
        // Arrange
        let a = vec![1.0f32; 9];
        let b = vec![2.0f32; 9];

        // Act
        let l2 = l2_distance(&a, &b);
        let cosine = cosine_distance(&a, &b);
        let dot = dot_score(&a, &b);

        // Assert
        assert_f64_close(l2, 3.0);
        assert_f64_close(cosine, 0.0);
        assert_f64_close(dot, 18.0);
    }

    #[test]
    fn should_return_sentinel_values_for_mismatched_vector_lengths() {
        // Arrange
        let a = vec![1.0f32, 2.0, 3.0];
        let b = vec![1.0f32, 2.0];

        // Act
        let l2 = l2_distance(&a, &b);
        let cosine = cosine_distance(&a, &b);
        let dot = dot_score(&a, &b);

        // Assert
        assert_f64_close(l2, f64::MAX);
        assert_f64_close(cosine, 1.0);
        assert_f64_close(dot, 0.0);
    }

    #[test]
    fn should_compute_hybrid_score_deterministically() {
        // Arrange
        let search_score = 0.2;
        let vector_score = 0.8;

        // Act
        let score = hybrid_score(search_score, vector_score, None);

        // Assert
        assert_f64_close(score, 0.41);
    }

    #[test]
    fn should_hybrid_score_use_custom_weights() {
        // Arrange
        let search_score = 0.2;
        let vector_score = 0.8;
        let policy = cassie::hybrid::HybridScorePolicy {
            search_weight: 0.25,
            vector_weight: 0.75,
        };

        // Act
        let score = hybrid_score(search_score, vector_score, Some(&policy));

        // Assert
        assert_f64_close(score, 0.65);
    }

    #[test]
    fn should_compute_tokenized_bm25_like_score_for_query_terms() {
        // Arrange
        let haystack = "The quick brown fox jumps over the lazy dog";
        let tokens = tokenizer::tokenize(haystack);

        // Act
        let tf_quick = usize_to_f64(tokens.iter().filter(|term| *term == "quick").count());
        let tf_dog = usize_to_f64(tokens.iter().filter(|term| *term == "dog").count());
        let dl = usize_to_f64(tokens.len());
        let avg_dl = dl;
        let k1 = 1.2;
        let b = 0.75;
        let n = 10.0;
        let score_common_term = bm25::bm25_score(tf_quick, 5.0, n, k1, b, dl, avg_dl);
        let score_rare_term = bm25::bm25_score(tf_dog, 1.0, n, k1, b, dl, avg_dl);
        let query_score = bm25::bm25_score(tf_quick, 5.0, n, k1, b, dl, avg_dl)
            + bm25::bm25_score(tf_dog, 1.0, n, k1, b, dl, avg_dl);
        let expected = score_common_term + score_rare_term;

        // Assert
        let observed = query_score;
        assert_f64_close(observed, expected);
        assert!(observed > score_common_term);
    }

    #[test]
    fn should_clamp_bm25_score_for_invalid_document_frequency() {
        // Arrange
        let tf = 1.0;
        let df = 10.0;
        let n = 1.0;
        let k1 = 1.2;
        let b = 0.75;
        let dl = 3.0;
        let avg_dl = 3.0;

        // Act
        let score = bm25::bm25_score(tf, df, n, k1, b, dl, avg_dl);

        // Assert
        assert!(score.is_finite());
        assert!(score >= 0.0);
    }

    #[test]
    fn should_generate_snippet_with_highlight_markup() {
        // Arrange
        let input = "Rust enables fast, reliable systems programming";
        let terms = vec!["rust".to_string(), "systems".to_string()];

        // Act
        let output = bm25::snippet(input, &terms);

        // Assert
        assert_eq!(
            output,
            "<mark>Rust</mark> enables fast, reliable <mark>systems</mark> programming"
        );
    }

    #[test]
    fn should_highlight_unicode_snippet_after_expanding_lowercase_character() {
        // Arrange
        let input = "İstanbul is nice";
        let terms = vec!["nice".to_string()];

        // Act
        let output = bm25::snippet(input, &terms);

        // Assert
        assert_eq!(output, "İstanbul is <mark>nice</mark>");
    }

    #[test]
    fn should_filter_stop_words_before_scoring_tokens() {
        // Arrange
        let input = "The quick brown fox and the lazy dog";

        // Act
        let tokens = tokenizer::tokenize(input);

        // Assert
        assert_eq!(tokens, vec!["quick", "brown", "fox", "lazy", "dog"]);
    }
}
