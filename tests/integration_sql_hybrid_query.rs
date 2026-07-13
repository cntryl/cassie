#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
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
fn should_order_hybrid_top_k_by_score_with_limit() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_top_k_limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_top_k_limit";
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
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_top_k_limit ORDER BY score DESC LIMIT 1",
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
fn should_generate_hybrid_candidates_from_text_matches() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_text_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_text_candidates";
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
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("text_match".to_string()),
                serde_json::json!({"body": "red", "embedding": [100.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("vector_only".to_string()),
                serde_json::json!({"body": "blue", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        let before = cassie.metrics();
        let before_candidates = before["hybrid"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_text_candidates ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("text_match".to_string()));
        assert_eq!(
            after["hybrid"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_mixed_text_vector_execution_stages() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_explain_mixed_stages");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_explain_mixed_stages";
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
        cassie.midge.create_collection(collection, schema.clone()).unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_explain_mixed_stages ORDER BY score DESC LIMIT 5",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("mixed_execution=true"));
        assert!(plan.contains("mixed_stages=candidate_generation>exact_scoring>ordering"));
        assert!(plan.contains(">limit"));
        assert!(plan.contains("exact_baseline=source_row_exact_baseline"));
        assert!(plan.contains("projection_freshness=unavailable"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_hybrid_text_candidate_without_vector() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_missing_vector");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_missing_vector";
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
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("text_without_vector".to_string()),
                serde_json::json!({"body": "red"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("ignored_non_match".to_string()),
                serde_json::json!({"body": "blue"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_missing_vector ORDER BY score DESC LIMIT 1",
                vec![],
            );

        // Assert
        let error = result.expect_err("text candidate should require a vector");
        assert!(error.to_string().contains("vector_score expects vector"));

        let _ = std::fs::remove_dir_all(path);
    });
}

fn bounded_hybrid_fixture() -> (Cassie, String, &'static str) {
    bounded_hybrid_fixture_with_max(100_000)
}

fn bounded_hybrid_fixture_with_max(max_candidates: usize) -> (Cassie, String, &'static str) {
    with_fallback();
    let path = data_dir("hybrid_bounded_candidates");
    let mut config = CassieRuntimeConfig::from_env().unwrap();
    config.limits.adaptive_candidate_max = max_candidates;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let collection = "hybrid_bounded_candidates";
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
    cassie.register_collection(
        collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    for index in 0..64 {
        cassie
            .midge
            .put_document(
                collection,
                Some(format!("d{index}")),
                serde_json::json!({
                    "body": if index < 2 { "alpha marker" } else { "unrelated" },
                    "embedding": [1.0, 0.0]
                }),
            )
            .unwrap();
    }
    let fulltext = IndexMeta {
        collection: collection.to_string(),
        name: "fulltext_body_idx".to_string(),
        field: "body".to_string(),
        fields: vec!["body".to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::FullText,
        unique: false,
        options: std::collections::BTreeMap::new(),
    };
    cassie.midge.put_index(&fulltext).unwrap();
    cassie.catalog.register_index(fulltext);
    cassie
        .midge
        .put_vector_index(VectorIndexRecord {
            collection: collection.to_string(),
            field: "embedding".to_string(),
            source_field: "body".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 2,
                metric: DistanceMetric::L2,
                index_type: VectorIndexType::IvfFlat,
                hnsw: None,
                hnsw_graph: None,
                ivfflat: Some(cassie::embeddings::IvfFlatIndexOptions {
                    version: 1,
                    lists: 2,
                    probes: 1,
                    training_sample_size: 64,
                    training_seed: 1,
                }),
                ivfflat_training: None,
            },
        })
        .unwrap();
    (cassie, path, collection)
}

fn corrupt_matching_data_value(cassie: &Cassie, predicate: impl Fn(&serde_json::Value) -> bool) {
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap()
        .into_iter()
        .find_map(|(key, raw)| {
            serde_json::from_slice::<serde_json::Value>(&raw)
                .ok()
                .filter(|value| predicate(value))
                .map(|_| key)
        })
        .expect("matching persisted artifact");
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    tx.put(key, b"corrupt".to_vec(), None).unwrap();
    tx.commit(WriteOptions::sync()).unwrap();
}

#[test]
fn should_bound_hybrid_reads_to_persisted_text_candidates() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture();
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0]')) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
    let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
        - before["storage"]["data"]["reads"].as_u64().unwrap();
    assert!(
        reads < 64,
        "expected bounded hybrid reads, observed {reads}"
    );
    assert!(
        after["hybrid"]["posting_reads_total"].as_u64().unwrap()
            > before["hybrid"]["posting_reads_total"].as_u64().unwrap()
    );
    assert!(
        after["hybrid"]["ann_reads_total"].as_u64().unwrap()
            > before["hybrid"]["ann_reads_total"].as_u64().unwrap()
    );
    assert!(
        after["hybrid"]["exact_reranks_total"].as_u64().unwrap()
            > before["hybrid"]["exact_reranks_total"].as_u64().unwrap()
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_when_hybrid_text_artifact_is_corrupt() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture();
    corrupt_matching_data_value(&cassie, |value| {
        value.get("index_name") == Some(&serde_json::json!("fulltext_body_idx"))
    });
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0]')) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
    assert_eq!(
        after["hybrid"]["prefilter_fallback_reasons"]["text-artifact"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["prefilter_fallback_reasons"]["text-artifact"]
                .as_u64()
                .unwrap_or_default(),
        1
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_when_hybrid_vector_artifact_is_corrupt() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture();
    corrupt_matching_data_value(&cassie, |value| {
        value.get("source_fingerprint").is_some()
            && value.get("row_count").is_some()
            && value.get("built_generation").is_some()
    });
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0]')) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
    assert_eq!(
        after["hybrid"]["prefilter_fallback_reasons"]["vector-artifact"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["prefilter_fallback_reasons"]["vector-artifact"]
                .as_u64()
                .unwrap_or_default(),
        1
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_hybrid_candidate_budget_rejection() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture_with_max(1);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0]')) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows[0][0], Value::String("d0".to_string()));
    assert_eq!(
        after["hybrid"]["candidate_budget_rejections_total"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["candidate_budget_rejections_total"]
                .as_u64()
                .unwrap_or_default(),
        1
    );
    assert_eq!(
        after["hybrid"]["truncation_count_total"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["truncation_count_total"]
                .as_u64()
                .unwrap_or_default(),
        1
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_bounded_hybrid_queries_concurrently() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture();
    let cassie = std::sync::Arc::new(cassie);

    // Act
    let handles = (0..4)
        .map(|_| {
            let cassie = std::sync::Arc::clone(&cassie);
            let collection = collection.to_string();
            std::thread::spawn(move || {
                let session = cassie.create_session("tester", None);
                cassie
                    .execute_sql(
                        &session,
                        &format!("SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0]')) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
                        vec![],
                    )
                    .unwrap()
                    .rows[0][0]
                    .clone()
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(results.len(), 4);
    assert!(results
        .into_iter()
        .all(|value| value == Value::String("d0".to_string())));
    let _ = std::fs::remove_dir_all(path);
}
