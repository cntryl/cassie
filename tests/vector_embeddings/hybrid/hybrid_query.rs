#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord,
    VectorIndexType, DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support_sql as support;
use support::*;

fn hybrid_params(query: &str) -> Vec<Value> {
    vec![
        Value::String(query.to_string()),
        Value::Vector(Vector::new(vec![1.0, 0.0])),
    ]
}

#[test]
fn should_order_hybrid_top_k_by_score_with_limit() {
    // Arrange
    let path = data_dir("hybrid_top_k_limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
                "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_top_k_limit ORDER BY score DESC LIMIT 1",
                hybrid_params("red"),
            )

.unwrap();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows()[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_generate_hybrid_candidates_from_text_matches() {
    // Arrange
    let path = data_dir("hybrid_text_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
                "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_text_candidates ORDER BY score DESC LIMIT 1",
                hybrid_params("red"),
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], Value::String("text_match".to_string()));
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
    let path = data_dir("hybrid_explain_mixed_stages");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
                "EXPLAIN SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_explain_mixed_stages ORDER BY score DESC LIMIT 5",
                hybrid_params("red"),
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows()[0][0] else {
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

fn setup_missing_vector_hybrid_fixture(cassie: &Cassie, collection: &str) {
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
    let fulltext_index = IndexMeta {
        collection: collection.to_string(),
        name: "sql_hybrid_missing_vector_body_idx".to_string(),
        field: "body".to_string(),
        fields: vec!["body".to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::FullText,
        unique: false,
        options: std::collections::BTreeMap::new(),
    };
    cassie
        .midge
        .put_index(&fulltext_index)
        .expect("persist fulltext index");
    cassie.catalog.register_index(fulltext_index);
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
    cassie
        .midge
        .put_document(
            collection,
            Some("scored_match".to_string()),
            serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
        )
        .unwrap();
}

fn missing_vector_hybrid_index(collection: &str) -> VectorIndexRecord {
    VectorIndexRecord {
        collection: collection.to_string(),
        field: "embedding".to_string(),
        source_field: "body".to_string(),
        metadata: VectorIndexMetadata {
            provider: "manual".to_string(),
            model: "manual".to_string(),
            dimensions: 2,
            metric: DistanceMetric::L2,
            index_type: VectorIndexType::Hnsw,
            hnsw: Some(HnswIndexOptions {
                version: 1,
                m: 2,
                ef_construction: 4,
                ef_search: 8,
            }),
            hnsw_graph: None,
            ivfflat: None,
            ivfflat_training: None,
        },
    }
}

fn execute_missing_vector_hybrid_query(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
) -> Vec<Vec<Value>> {
    cassie
            .execute_sql(
                session,
                "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM sql_hybrid_missing_vector ORDER BY score DESC LIMIT 1",
                hybrid_params("red"),
            )
            .expect("hybrid query")
            .rows().to_vec()
}

#[test]
fn should_skip_hybrid_text_candidate_without_vector() {
    // Arrange
    let path = data_dir("hybrid_missing_vector");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        let collection = "sql_hybrid_missing_vector";
        setup_missing_vector_hybrid_fixture(&cassie, collection);
        let session = cassie.create_session("tester", None);

        // Act
        let fallback_rows = execute_missing_vector_hybrid_query(&cassie, &session);

        // Assert
        assert_eq!(fallback_rows.len(), 1);
        assert_eq!(
            fallback_rows[0][0],
            Value::String("scored_match".to_string())
        );

        let vector_index = missing_vector_hybrid_index(collection);
        cassie
            .midge
            .put_vector_index(vector_index.clone())
            .expect("build vector index");
        cassie.register_vector_index(vector_index);
        let indexed_rows = execute_missing_vector_hybrid_query(&cassie, &session);

        // Assert
        assert_eq!(indexed_rows.len(), fallback_rows.len());
        assert_eq!(indexed_rows[0][0], fallback_rows[0][0]);

        let _ = std::fs::remove_dir_all(path);
    });
}

fn bounded_hybrid_fixture() -> (Cassie, String, &'static str) {
    bounded_hybrid_fixture_with_limits(100_000, None)
}

fn bounded_hybrid_fixture_with_max(max_candidates: usize) -> (Cassie, String, &'static str) {
    bounded_hybrid_fixture_with_limits(max_candidates, None)
}

fn bounded_hybrid_fixture_with_limits(
    max_candidates: usize,
    query_memory_budget_bytes: Option<usize>,
) -> (Cassie, String, &'static str) {
    let path = data_dir("hybrid_bounded_candidates");
    let mut config = CassieRuntimeConfig::from_env().unwrap();
    config.limits.adaptive_candidate_max = max_candidates;
    if let Some(query_memory_budget_bytes) = query_memory_budget_bytes {
        config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
    }
    let cassie = Cassie::new_in_memory_with_config(config).unwrap();
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
                    "embedding": [1.0, 0.0],
                    "status": if index == 0 { "approved" } else { "pending" }
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

fn corrupt_fulltext_artifact(cassie: &Cassie, collection: &str, index_name: &str) {
    let prefix = cassie
        .midge
        .fulltext_artifact_prefix_for_diagnostics(collection, index_name)
        .unwrap();
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .unwrap()
        .into_iter()
        .find_map(|(key, value)| value.starts_with(b"FTM1").then_some(key))
        .expect("full-text manifest artifact");
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
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows()[0][0], Value::String("d0".to_string()));
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
fn should_push_structured_filter_over_bounded_hybrid_candidates() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture();
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} WHERE status = $3 ORDER BY score DESC LIMIT 1"),
            {
                let mut params = hybrid_params("alpha");
                params.push(Value::String("approved".to_string()));
                params
            },
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows()[0][0], Value::String("d0".to_string()));
    let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
        - before["storage"]["data"]["reads"].as_u64().unwrap();
    assert!(
        reads < 64,
        "expected bounded filtered reads, observed {reads}"
    );
    assert!(
        after["hybrid"]["prefilter_filtered_candidate_count_total"]
            .as_u64()
            .unwrap()
            > before["hybrid"]["prefilter_filtered_candidate_count_total"]
                .as_u64()
                .unwrap()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_when_hybrid_text_artifact_is_corrupt() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture();
    corrupt_fulltext_artifact(&cassie, collection, "fulltext_body_idx");
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows()[0][0], Value::String("d0".to_string()));
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
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows()[0][0], Value::String("d0".to_string()));
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
fn should_bound_hybrid_candidates_before_fulltext_stat_fetch() {
    // Arrange
    const PHYSICAL_ANN_READ_BOUND: u64 = 2 * 64 + 2;
    let (cassie, path, collection) = bounded_hybrid_fixture_with_max(1);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows()[0][0], Value::String("d0".to_string()));
    assert_eq!(
        after["hybrid"]["candidate_budget_rejections_total"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["candidate_budget_rejections_total"]
                .as_u64()
                .unwrap_or_default(),
        0
    );
    assert_eq!(
        after["hybrid"]["truncation_count_total"]
            .as_u64()
            .unwrap_or_default()
            - before["hybrid"]["truncation_count_total"]
                .as_u64()
                .unwrap_or_default(),
        0
    );
    let ann_reads = after["hybrid"]["ann_reads_total"]
        .as_u64()
        .unwrap_or_default()
        - before["hybrid"]["ann_reads_total"]
            .as_u64()
            .unwrap_or_default();
    let candidates = after["hybrid"]["candidate_count_total"]
        .as_u64()
        .unwrap_or_default()
        - before["hybrid"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
    assert!(candidates <= 1, "observed {candidates} hybrid candidates");
    assert!(
        ann_reads <= PHYSICAL_ANN_READ_BOUND,
        "observed {ann_reads} controlled ANN reads"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enforce_query_memory_budget_for_hybrid_candidates() {
    // Arrange
    let (cassie, path, collection) = bounded_hybrid_fixture_with_limits(100_000, Some(16));
    let session = cassie.create_session("tester", None);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
            hybrid_params("alpha"),
        )
        .expect_err("hybrid candidates should exceed the query memory budget");

    // Assert
    assert!(
        error.to_string().contains("query memory budget"),
        "unexpected error: {error}"
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
                        &format!("SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM {collection} ORDER BY score DESC LIMIT 1"),
                        hybrid_params("alpha"),
                    )
                    .unwrap()
                    .rows()[0][0]
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
