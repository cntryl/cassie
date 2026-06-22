#![allow(unused_imports, dead_code)]
use cassie::app::{Cassie, CassieSession};
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::runtime::{RuntimeFeedbackKey, RuntimeFeedbackObservation};
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

fn feedback_key(
    cassie: &Cassie,
    session: &CassieSession,
    sql: &str,
    candidate_index: Option<&str>,
) -> RuntimeFeedbackKey {
    cassie
        .read_operator_feedback_key_for_diagnostics(session, sql, candidate_index)
        .expect("feedback key")
}

fn adaptive_execution_config(
    enabled: bool,
    min_cost_savings_bps: usize,
) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env();
    config.limits.operator_feedback_enabled = true;
    config.limits.adaptive_execution_enabled = enabled;
    config.limits.adaptive_min_cost_savings_bps = min_cost_savings_bps;
    config
}

fn confident_feedback(elapsed_ms: u64, storage_reads: u64) -> RuntimeFeedbackObservation {
    RuntimeFeedbackObservation {
        rows_in: storage_reads.max(1),
        rows_out: 1,
        elapsed_ms,
        storage_reads,
        ..RuntimeFeedbackObservation::default()
    }
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

fn register_operator_feedback_indexes(
    cassie: &Cassie,
    collection: &str,
    first_index: &str,
    second_index: &str,
) {
    for (field, index_name) in [("body", first_index), ("title", second_index)] {
        let index = IndexMeta {
            collection: collection.to_string(),
            name: index_name.to_string(),
            field: field.to_string(),
            fields: vec![field.to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: Default::default(),
        };
        cassie.midge.put_index(index.clone()).unwrap();
        cassie.catalog.register_index(index);
    }
}

fn adaptive_candidate_config(min: usize, max: usize) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env();
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
fn should_record_adaptive_candidate_expansion() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_expansion");
    let config = adaptive_candidate_config(1, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_expansion";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_expansion ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["adaptive_candidates"]["decisions"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["decisions"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["initial_budget_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["initial_budget_total"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["final_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["final_candidate_count_total"]
                    .as_u64()
                    .unwrap_or_default(),
            3
        );
        assert!(
            after["adaptive_candidates"]["expansions_total"]
                .as_u64()
                .unwrap_or_default()
                > before["adaptive_candidates"]["expansions_total"]
                    .as_u64()
                    .unwrap_or_default(),
            "candidate work beyond the initial budget should be counted"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_adaptive_candidate_cap_overflow() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_cap");
    let config = adaptive_candidate_config(1, 1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_cap";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_cap ORDER BY score DESC LIMIT 2",
                vec![],
            )
            .expect_err("query should exceed adaptive candidate cap");
        let after = cassie.metrics();

        // Assert
        assert!(
            error
                .to_string()
                .contains("adaptive candidate max"),
            "error should name the adaptive candidate cap: {error}"
        );
        assert_eq!(
            after["adaptive_candidates"]["limit_errors_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["limit_errors_total"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_adaptive_candidate_budget_in_explain() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_explain");
    let config = adaptive_candidate_config(2, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_explain";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_explain ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = result.rows[0][0].as_str().unwrap_or_default();
        assert!(
            plan.contains("candidate_budget=2"),
            "explain should include adaptive candidate budget: {plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_select_adaptive_read_operator_alternative() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_read_operator_select");
    let config = adaptive_execution_config(true, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_read_operator_select";
        let base_index = "metrics_adaptive_read_operator_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_select WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN ANALYZE SELECT title FROM metrics_adaptive_read_operator_select WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(base_key.clone(), confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(preferred_key.clone(), confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }
        let before = cassie.metrics();

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();
        let after = cassie.metrics();

        // Assert
        assert!(plan.contains(preferred_index), "plan={plan}");
        assert!(plan.contains("adaptive_plan_enabled=true"), "plan={plan}");
        assert!(
            plan.contains(&format!(
                "adaptive_selected_alternative=index:{preferred_index}"
            )),
            "plan={plan}"
        );
        assert!(
            plan.contains("adaptive_reason=selected_operator_feedback"),
            "plan={plan}"
        );
        assert!(
            plan.contains("adaptive_plan_decisions_delta:1"),
            "plan={plan}"
        );
        assert!(
            plan.contains("adaptive_plan_selected_delta:1"),
            "plan={plan}"
        );
        assert_eq!(
            after["adaptive_candidates"]["plan_selected_alternatives"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["plan_selected_alternatives"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_plan_selected_alternative"],
            format!("index:{preferred_index}")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_base_alternative_when_adaptive_guard_fails() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_read_operator_guard");
    let config = adaptive_execution_config(true, 10_000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_read_operator_guard";
        let base_index = "metrics_adaptive_read_operator_guard_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_guard_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_guard WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN SELECT title FROM metrics_adaptive_read_operator_guard WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(base_key.clone(), confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(preferred_key.clone(), confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();

        // Assert
        assert!(plan.contains(base_index), "plan={plan}");
        assert!(
            plan.contains(&format!("adaptive_selected_alternative=index:{base_index}")),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_guard_passed=false"), "plan={plan}");
        assert!(plan.contains("adaptive_reason=guard_failed"), "plan={plan}");

        let _ = std::fs::remove_dir_all(path);
    });
}
