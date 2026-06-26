#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::runtime::RuntimeFeedbackKey;
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-metrics-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn startup_frame(user: &str, database: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);
    payload.push(0);

    let mut frame = Vec::new();
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn feedback_key(sql: &str, collection: &str, schema_epoch: u64) -> RuntimeFeedbackKey {
    let _ = (sql, collection, schema_epoch);
    panic!("feedback_key helper is unused in metrics_search");
}

fn register_feedback_collection(cassie: &Cassie, collection: &str) {
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
    cassie.register_collection(collection, schema);
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha", "body": "one"}),
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
}

fn adaptive_candidate_config(min: usize, max: usize) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.adaptive_candidate_min = min;
    config.limits.adaptive_candidate_max = max;
    config
}

fn register_adaptive_candidate_collection(cassie: &Cassie, collection: &str) {
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
    cassie.register_collection(collection, schema);
    for (id, body) in [
        ("doc-1", "alpha shared"),
        ("doc-2", "alpha shared"),
        ("doc-3", "alpha shared"),
    ] {
        cassie
            .midge
            .put_document(
                collection,
                Some(id.to_string()),
                serde_json::json!({"body": body}),
            )
            .unwrap();
    }
}

fn describe_statement_frame(statement_name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(b'S');
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'D');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("describe payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn read_auth_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, i32, Vec<u8>) {
    let mut header = [0u8; 5];
    tokio::io::AsyncReadExt::read_exact(reader, &mut header)
        .await
        .expect("read auth frame header");

    let tag = header[0];
    let len = i32::from_be_bytes(header[1..].try_into().expect("auth frame length"));
    let mut payload =
        vec![0u8; usize::try_from(len - 4).expect("non-negative auth payload length")];
    tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
        .await
        .expect("read auth frame payload");

    (tag, len, payload)
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, Vec<u8>) {
    let mut tag = [0u8; 1];
    tokio::io::AsyncReadExt::read_exact(reader, &mut tag)
        .await
        .expect("read frame tag");

    let mut len = [0u8; 4];
    tokio::io::AsyncReadExt::read_exact(reader, &mut len)
        .await
        .expect("read frame length");
    let len = i32::from_be_bytes(len);
    let mut payload = vec![0u8; usize::try_from(len - 4).expect("non-negative payload length")];
    if !payload.is_empty() {
        tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
            .await
            .expect("read frame payload");
    }

    (tag[0], payload)
}

#[test]
fn should_record_vector_counts_for_ordered_search_expression() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_vector_candidates";
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
                Some("doc-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "embedding": [1.0, 0.0],
                }),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({
                    "title": "beta",
                    "embedding": [0.0, 1.0],
                }),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({
                    "title": "gamma",
                    "embedding": [1.0, 1.0],
                }),
            )

            .unwrap();

        let before = cassie.metrics();
        let before_candidates = before["vector"]["candidate_count_total"].as_u64().unwrap_or_default();
        let before_results = before["vector"]["result_count_total"].as_u64().unwrap_or_default();

        let session = cassie.create_session("tester", None);
        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_vector_candidates ORDER BY embedding <-> '[1,0]' LIMIT 1",
                vec![],
            )

.unwrap();

        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["vector"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            3
        );
        assert_eq!(
            after["vector"]["result_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_results,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_vector_prefilter_candidate_counts() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_prefilter_counts");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_vector_prefilter_counts";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "status".to_string(),
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
                Some("doc-1".to_string()),
                serde_json::json!({
                    "status": "approved",
                    "embedding": [1.0, 0.0],
                }),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({
                    "status": "approved",
                    "embedding": [2.0, 0.0],
                }),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({
                    "status": "pending",
                    "embedding": [3.0, 0.0],
                }),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_input = before["vector"]["prefilter_input_candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_filtered = before["vector"]["prefilter_filtered_candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_fallback = before["vector"]["prefilter_fallback_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM metrics_vector_prefilter_counts WHERE status = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["vector"]["prefilter_input_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_input,
            3
        );
        assert_eq!(
            after["vector"]["prefilter_filtered_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_filtered,
            2
        );
        assert_eq!(
            after["vector"]["prefilter_fallback_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_fallback,
            0
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_search_operator_statistics() {
    // Arrange
    with_fallback();
    let path = data_dir("search_operator_stats");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_search_operator_stats";
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
                Some("doc-1".to_string()),
                serde_json::json!({"body": "alpha bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"body": "alpha charlie"}),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_candidates = before["search"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_results = before["search"]["result_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_search_operator_stats ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["search"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            2
        );
        assert_eq!(
            after["search"]["result_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_results,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_search_operator_candidates_after_posting_list_filtering() {
    // Arrange
    with_fallback();
    let path = data_dir("search_operator_posting_list_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_search_operator_posting_list_candidates";
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
                Some("doc-1".to_string()),
                serde_json::json!({"body": "alpha bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"body": "bravo charlie"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({"body": "charlie delta"}),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_candidates = before["search"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_results = before["search"]["result_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_search_operator_posting_list_candidates WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["search"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            1
        );
        assert_eq!(
            after["search"]["result_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_results,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_candidate_tie_order() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_ties");
    let config = adaptive_candidate_config(1, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_ties";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_ties ORDER BY score DESC LIMIT 3",
                vec![],
            )
            .unwrap();

        // Assert
        let ids = result
            .rows
            .iter()
            .map(|row| row[0].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["doc-1", "doc-2", "doc-3"]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cache_fulltext_scoring_metadata_for_repeated_search_queries() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_scoring_metadata_cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_fulltext_scoring_metadata_cache";
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
                Some("doc-1".to_string()),
                serde_json::json!({"body": "alpha bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"body": "alpha charlie"}),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_hits = before["query_cache"]["fulltext_stats_hits"]
            .as_u64()
            .unwrap_or_default();
        let before_misses = before["query_cache"]["fulltext_stats_misses"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_fulltext_scoring_metadata_cache WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after_first = cassie.metrics();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_fulltext_scoring_metadata_cache WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let after_second = cassie.metrics();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_fulltext_scoring_metadata_cache (body) VALUES ('alpha delta')",
                vec![],
            )
            .unwrap();
        let third = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_fulltext_scoring_metadata_cache WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after_third = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 2);
        assert_eq!(third.rows.len(), 1);
        assert_eq!(
            after_first["query_cache"]["fulltext_stats_misses"]
                .as_u64()
                .unwrap_or_default()
                - before_misses,
            1
        );
        assert_eq!(
            after_first["query_cache"]["fulltext_stats_hits"]
                .as_u64()
                .unwrap_or_default()
                - before_hits,
            0
        );
        assert_eq!(
            after_second["query_cache"]["fulltext_stats_hits"]
                .as_u64()
                .unwrap_or_default()
                - before_hits,
            1
        );
        assert_eq!(
            after_second["query_cache"]["fulltext_stats_misses"]
                .as_u64()
                .unwrap_or_default()
                - before_misses,
            1
        );
        assert_eq!(
            after_third["query_cache"]["fulltext_stats_hits"]
                .as_u64()
                .unwrap_or_default()
                - before_hits,
            1
        );
        assert_eq!(
            after_third["query_cache"]["fulltext_stats_misses"]
                .as_u64()
                .unwrap_or_default()
                - before_misses,
            2
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
