// Consolidated integration suite: metrics.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/adaptive_evidence.rs"]
mod support_adaptive_evidence;
#[path = "support/metrics.rs"]
mod support_metrics;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/metrics_adaptive.rs.
mod metrics_adaptive {
    #![allow(unused_imports, dead_code)]

    use super::support_adaptive_evidence::AdaptiveProfileEvidence;
    use super::support_pgwire as pgwire_support;

    use cassie::app::{Cassie, CassieSession};
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::OperatorSwitchingEnabled;
    use cassie::runtime::{
        QueryCancellationHandle, RuntimeFeedbackKey, RuntimeFeedbackObservation,
    };
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{data_dir, describe_statement_frame, startup_frame, use_local_storage};

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
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.operator_feedback_enabled = true;
        config.limits.adaptive_execution_enabled = enabled;
        config.limits.adaptive_min_cost_savings_bps = min_cost_savings_bps;
        config
    }

    fn adaptive_execution_confidence_config(
        enabled: bool,
        min_cost_savings_bps: usize,
        min_confidence_bps: u16,
    ) -> cassie::config::CassieRuntimeConfig {
        let mut config = adaptive_execution_config(enabled, min_cost_savings_bps);
        config.limits.adaptive_min_confidence_bps = min_confidence_bps;
        config
    }

    fn snapshot_delta(after: &serde_json::Value, before: &serde_json::Value, path: &[&str]) -> u64 {
        let after_value = path.iter().fold(after, |value, key| &value[*key]);
        let before_value = path.iter().fold(before, |value, key| &value[*key]);
        after_value.as_u64().unwrap_or_default() - before_value.as_u64().unwrap_or_default()
    }

    fn operator_switch_config(
        enabled: bool,
        threshold: usize,
    ) -> cassie::config::CassieRuntimeConfig {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.vectorized_joins_enabled = true;
        config.limits.vectorized_join_batch_size = 2;
        config.limits.operator_switching_enabled = if enabled {
            OperatorSwitchingEnabled::enabled()
        } else {
            OperatorSwitchingEnabled::disabled()
        };
        config.limits.operator_switch_join_row_threshold = threshold;
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
                options: std::collections::BTreeMap::default(),
            };
            cassie.midge.put_index(&index).unwrap();
            cassie.catalog.register_index(index);
        }
    }

    fn create_switch_join_tables(
        cassie: &Cassie,
        session: &CassieSession,
        users: &str,
        orders: &str,
    ) {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {users} (user_key INT, name TEXT)"),
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {orders} (order_user_key INT, total INT)"),
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {users} (user_key, name) VALUES (1, 'ada'), (2, 'grace')"),
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {orders} (order_user_key, total) VALUES (1, 10), (2, 20)"),
                vec![],
            )
            .unwrap();
    }

    fn create_persisted_adaptive_fixture(
        cassie: &Cassie,
        session: &CassieSession,
        collection: &str,
        base_index: &str,
        preferred_index: &str,
    ) {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {collection} (title TEXT, body TEXT)"),
                vec![],
            )
            .unwrap();
        let rows = std::iter::repeat_n("('alpha', 'one')", 8)
            .chain(std::iter::once("('beta', 'two')"))
            .collect::<Vec<_>>()
            .join(", ");
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {collection} (title, body) VALUES {rows}"),
                vec![],
            )
            .unwrap();
        for (index, field) in [(base_index, "body"), (preferred_index, "title")] {
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE INDEX {index} ON {collection} USING btree ({field})"),
                    vec![],
                )
                .unwrap();
        }
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
        pgwire_support::read_wire_frame(reader).await
    }

    #[test]
    fn should_record_adaptive_candidate_expansion() {
        // Arrange
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
    fn should_preserve_the_selected_adaptive_read_surface_across_profiles() {
        // Arrange
        let evidence = AdaptiveProfileEvidence::collect();

        // Act
        evidence.write_requested_artifact();

        // Assert
        evidence.assert_equivalent();
    }

    #[test]
    fn should_select_adaptive_read_operator_alternative() {
        // Arrange
        use_local_storage();
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
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
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
    fn should_report_adaptive_read_operator_disabled_without_selected_delta() {
        // Arrange
        use_local_storage();
        let path = data_dir("adaptive_read_operator_disabled");
        let config = adaptive_execution_config(false, 0);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_read_operator_disabled";
        let base_index = "metrics_adaptive_read_operator_disabled_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_disabled_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_disabled WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN ANALYZE SELECT title FROM metrics_adaptive_read_operator_disabled WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }
        let before = cassie.metrics();

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();
        let after = cassie.metrics();

        // Assert
        assert!(plan.contains("operator_feedback=used"), "plan={plan}");
        assert!(
            plan.contains(&format!(
                "operator_feedback_selected_candidate=index:{preferred_index}"
            )),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_plan_enabled=false"), "plan={plan}");
        assert!(
            plan.contains(&format!("adaptive_base_alternative=index:{base_index}")),
            "plan={plan}"
        );
        assert!(
            plan.contains(&format!("adaptive_selected_alternative=index:{base_index}")),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_reason=disabled"), "plan={plan}");
        assert!(
            plan.contains("adaptive_plan_selected_delta:0"),
            "plan={plan}"
        );
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "plan_disabled_total"]
            ),
            1
        );
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "plan_selected_alternatives"]
            ),
            0
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_restore_fixed_plan_after_restart_without_deleting_persisted_feedback() {
        // Arrange
        use_local_storage();
        let path = data_dir("adaptive_profile_rollback");
        let mut evaluation = adaptive_execution_confidence_config(true, 500, 900);
        evaluation.limits.operator_switching_enabled = OperatorSwitchingEnabled::enabled();
        evaluation.limits.operator_switch_join_row_threshold = 4_096;
        let collection = "metrics_adaptive_profile_rollback";
        let base_index = "metrics_adaptive_profile_rollback_body_idx_a";
        let preferred_index = "metrics_adaptive_profile_rollback_title_idx_b";
        let shape_sql = "SELECT title FROM metrics_adaptive_profile_rollback WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN SELECT title FROM metrics_adaptive_profile_rollback WHERE title = 'alpha' AND body = 'one'";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, evaluation).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_persisted_adaptive_fixture(
                &cassie,
                &session,
                collection,
                base_index,
                preferred_index,
            );
            let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
            let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
            for _ in 0..4 {
                cassie
                    .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                    .expect("seed base feedback");
                cassie
                    .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                    .expect("seed preferred feedback");
            }
            let evaluation_plan = cassie
                .execute_sql(&session, explain_sql, vec![])
                .unwrap()
                .rows[0][0]
                .as_str()
                .unwrap()
                .to_string();
            let persisted_before = cassie
                .midge
                .list_runtime_feedback_records()
                .unwrap()
                .into_iter()
                .map(|(key, record)| (key, record.executions, record.stable_samples))
                .collect::<Vec<_>>();
            drop(session);
            drop(cassie);

            let mut disabled = cassie::config::CassieRuntimeConfig::from_env().unwrap();
            disabled.limits.operator_feedback_enabled = false;
            disabled.limits.adaptive_execution_enabled = false;
            disabled.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
            let cassie = Cassie::new_with_data_dir_and_config(&path, disabled).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let selected = cassie.execute_sql(&session, shape_sql, vec![]).unwrap();
            let disabled_plan = cassie
                .execute_sql(&session, explain_sql, vec![])
                .unwrap()
                .rows[0][0]
                .as_str()
                .unwrap()
                .to_string();
            let persisted_after = cassie
                .midge
                .list_runtime_feedback_records()
                .unwrap()
                .into_iter()
                .map(|(key, record)| (key, record.executions, record.stable_samples))
                .collect::<Vec<_>>();

            // Assert
            assert!(
                evaluation_plan.contains(&format!(
                    "adaptive_selected_alternative=index:postgres.public.{preferred_index}"
                )),
                "plan={evaluation_plan}"
            );
            assert_eq!(selected.rows.len(), 8);
            assert!(selected
                .rows
                .iter()
                .all(|row| { row == &vec![cassie::types::Value::String("alpha".into())] }));
            assert!(disabled_plan.contains("operator_feedback_reason=disabled"));
            assert!(disabled_plan.contains("adaptive_plan_enabled=false"));
            assert!(disabled_plan.contains(&format!(
                "adaptive_selected_alternative=index:postgres.public.{base_index}"
            )));
            assert_eq!(persisted_after.len(), persisted_before.len());
            for (key, executions, stable_samples) in &persisted_before {
                let (_, after_executions, after_stable_samples) = persisted_after
                    .iter()
                    .find(|(after_key, _, _)| after_key == key)
                    .expect("rollback must preserve every persisted feedback key");
                assert!(*after_executions >= *executions);
                assert!(*after_stable_samples >= *stable_samples);
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_base_alternative_when_adaptive_guard_fails() {
        // Arrange
        use_local_storage();
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
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
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

    #[test]
    fn should_keep_base_alternative_when_confidence_guard_fails() {
        // Arrange
        use_local_storage();
        let path = data_dir("adaptive_read_operator_confidence");
        let config = adaptive_execution_confidence_config(true, 100, 900);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_read_operator_confidence";
        let base_index = "metrics_adaptive_read_operator_confidence_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_confidence_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_confidence WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN SELECT title FROM metrics_adaptive_read_operator_confidence WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..3 {
            cassie
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
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
        assert!(
            plan.contains("adaptive_guard=operator_feedback_confidence_bps:750>=900"),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_guard_passed=false"), "plan={plan}");
        assert!(
            plan.contains("adaptive_reason=confidence_guard_failed"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_switch_vectorized_join_to_merge_when_threshold_exceeded() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_switch_join_threshold");
        let config = operator_switch_config(true, 2);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let session = cassie.create_session("tester", None);
        create_switch_join_tables(
            &cassie,
            &session,
            "metrics_switch_users",
            "metrics_switch_orders",
        );
        let before = cassie.metrics();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_switch_users.name, metrics_switch_orders.total FROM metrics_switch_users JOIN metrics_switch_orders ON metrics_switch_users.user_key = metrics_switch_orders.order_user_key ORDER BY metrics_switch_users.name",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    cassie::types::Value::String("ada".to_string()),
                    cassie::types::Value::Int64(10)
                ],
                vec![
                    cassie::types::Value::String("grace".to_string()),
                    cassie::types::Value::Int64(20)
                ],
            ]
        );
        assert_eq!(
            after["adaptive_candidates"]["operator_switch_successes"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["operator_switch_successes"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_pair"],
            "vectorized_join_to_merge_join"
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_reason"],
            "row_threshold_exceeded"
        );
        assert_eq!(after["joins"]["last_strategy"], "merge");
        assert_eq!(after["joins"]["vectorized_joins"], 0);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_report_operator_switch_failure_without_claiming_success() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_switch_failure");
        let mut config = operator_switch_config(true, 0);
        config.limits.query_memory_budget_bytes = 1_200;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let session = cassie.create_session("tester", None);
        create_switch_join_tables(
            &cassie,
            &session,
            "metrics_switch_failure_users",
            "metrics_switch_failure_orders",
        );
        let before = cassie.metrics();

        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT metrics_switch_failure_users.name, metrics_switch_failure_orders.total FROM metrics_switch_failure_users JOIN metrics_switch_failure_orders ON metrics_switch_failure_users.user_key = metrics_switch_failure_orders.order_user_key",
            vec![],
        );
        let after = cassie.metrics();

        // Assert
        assert!(result.is_err(), "the replacement operator must exhaust the fixture budget");
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "operator_switch_successes"]
            ),
            0
        );
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "operator_switch_fallbacks"]
            ),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_reason"],
            "replacement_failed"
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_cancel_before_operator_switch_without_leaking_resources_or_success() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_switch_cancellation");
        let mut config = operator_switch_config(true, 0);
        config.limits.max_query_workers = 1;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let session = cassie.create_session("tester", None);
            create_switch_join_tables(
                &cassie,
                &session,
                "metrics_switch_cancel_users",
                "metrics_switch_cancel_orders",
            );
            let cancellation = QueryCancellationHandle::new();
            cancellation.cancel();
            let before = cassie.metrics();

            // Act
            let result = cassie.execute_sql_with_cancellation(
                &session,
                "SELECT metrics_switch_cancel_users.name, metrics_switch_cancel_orders.total FROM metrics_switch_cancel_users JOIN metrics_switch_cancel_orders ON metrics_switch_cancel_users.user_key = metrics_switch_cancel_orders.order_user_key",
                vec![],
                &cancellation,
            );
            let after = cassie.metrics();

            // Assert
            assert!(matches!(result, Err(cassie::CassieError::QueryCancelled)));
            assert_eq!(
                snapshot_delta(
                    &after,
                    &before,
                    &["adaptive_candidates", "operator_switch_successes"]
                ),
                0
            );
            assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
            assert_eq!(after["runtime"]["active_operator_workers"], 0);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_vectorized_join_when_operator_switching_disabled() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_switch_disabled");
        let config = operator_switch_config(false, 0);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let session = cassie.create_session("tester", None);
        create_switch_join_tables(
            &cassie,
            &session,
            "metrics_switch_disabled_users",
            "metrics_switch_disabled_orders",
        );
        let before = cassie.metrics();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_switch_disabled_users.name, metrics_switch_disabled_orders.total FROM metrics_switch_disabled_users JOIN metrics_switch_disabled_orders ON metrics_switch_disabled_users.user_key = metrics_switch_disabled_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(selected.rows.len(), 2);
        assert_eq!(
            after["adaptive_candidates"]["operator_switch_attempts"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["operator_switch_attempts"]
                    .as_u64()
                    .unwrap_or_default(),
            0
        );
        assert_eq!(after["joins"]["last_strategy"], "vectorized");
        assert_eq!(after["joins"]["vectorized_joins"], 1);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_report_runtime_operator_switch_in_explain_analyze() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_switch_explain");
        let config = operator_switch_config(true, 1);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let session = cassie.create_session("tester", None);
        create_switch_join_tables(
            &cassie,
            &session,
            "metrics_switch_explain_users",
            "metrics_switch_explain_orders",
        );

        // Act
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN ANALYZE SELECT metrics_switch_explain_users.name, metrics_switch_explain_orders.total FROM metrics_switch_explain_users JOIN metrics_switch_explain_orders ON metrics_switch_explain_users.user_key = metrics_switch_explain_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let plan = explained.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(plan.contains("operator_switch_enabled=true"), "plan={plan}");
        assert!(
            plan.contains("operator_switch_pair=vectorized_join_to_merge_join"),
            "plan={plan}"
        );
        assert!(plan.contains("operator_switch_reason=armed"), "plan={plan}");
        assert!(
            plan.contains("operator_switch_success_delta:1"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_skip_runtime_operator_switch_for_unsupported_join_type() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_switch_unsupported");
        let config = operator_switch_config(true, 0);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let session = cassie.create_session("tester", None);
        create_switch_join_tables(
            &cassie,
            &session,
            "metrics_switch_right_users",
            "metrics_switch_right_orders",
        );
        let before = cassie.metrics();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_switch_right_users.name, metrics_switch_right_orders.total FROM metrics_switch_right_users RIGHT JOIN metrics_switch_right_orders ON metrics_switch_right_users.user_key = metrics_switch_right_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(selected.rows.len(), 2);
        assert_eq!(
            after["adaptive_candidates"]["operator_switch_skips"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["operator_switch_skips"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_reason"],
            "unsupported_join_type"
        );
        assert_eq!(after["joins"]["last_strategy"], "merge");

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/metrics_capacity.rs.
mod metrics_capacity {
    use cassie::app::Cassie;
    use cassie::embeddings::{
        DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    };
    use cassie::types::{Value, Vector};

    use super::support_metrics as support;
    use support::*;

    type MetricsValue = serde_json::Value;

    fn seed_capacity_fixture(cassie: &Cassie, session: &cassie::app::CassieSession) {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE metrics_capacity_docs (title TEXT, body TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("metrics_capacity_docs")
            .expect("catalog collection")
            .collection;
        cassie
        .execute_sql(
            session,
            "INSERT INTO metrics_capacity_docs (title, body, embedding) VALUES ('alpha', 'one two', $1)",
            vec![Value::Vector(Vector::new(vec![3.0, 4.0, 0.0]))],
        )
        .unwrap();
        cassie
        .execute_sql(
            session,
            "CREATE INDEX metrics_capacity_title_idx ON metrics_capacity_docs USING btree (title)",
            vec![],
        )
        .unwrap();
        cassie
        .execute_sql(
            session,
            "CREATE INDEX metrics_capacity_body_idx ON metrics_capacity_docs USING fulltext (body)",
            vec![],
        )
        .unwrap();
        cassie
        .execute_sql(
            session,
            "CREATE INDEX metrics_capacity_column_idx ON metrics_capacity_docs USING column (title, body) WITH (segment_size = 1)",
            vec![],
        )
        .unwrap();
        cassie
            .midge
            .put_vector_index(VectorIndexRecord {
                collection,
                field: "embedding".to_string(),
                source_field: "body".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::Cosine,
                    index_type: VectorIndexType::BruteForce,
                    hnsw: None,
                    hnsw_graph: None,
                    ivfflat: None,
                    ivfflat_training: None,
                },
            })
            .unwrap();
    }

    fn assert_capacity_metadata(capacity: &MetricsValue) {
        assert_eq!(capacity["advisory"], true);
        assert_eq!(capacity["local_only"], true);
        assert_eq!(capacity["persisted_metadata"], false);
        assert!(capacity["total_bytes"].as_u64().unwrap() > 0);
    }

    fn assert_capacity_families(capacity: &MetricsValue) {
        assert_eq!(capacity["families"]["schema"]["supported"], true);
        assert_eq!(capacity["families"]["data"]["supported"], true);
        assert_eq!(capacity["families"]["temp"]["supported"], true);
        assert_eq!(capacity["families"]["default"]["supported"], true);
        assert!(
            capacity["families"]["schema"]["total_bytes"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            capacity["families"]["data"]["total_bytes"]
                .as_u64()
                .unwrap()
                > 0
        );
    }

    fn assert_capacity_categories(capacity: &MetricsValue) {
        for category in [
            "row_blobs",
            "scalar_indexes",
            "fulltext",
            "vector_sidecars",
            "column_batches",
            "projection_metadata",
            "temp_artifacts",
            "other",
        ] {
            assert_eq!(capacity["categories"][category]["supported"], true);
            assert!(capacity["categories"][category]["total_bytes"]
                .as_u64()
                .is_some());
        }

        for category in [
            "row_blobs",
            "scalar_indexes",
            "fulltext",
            "vector_sidecars",
            "column_batches",
        ] {
            assert!(
                capacity["categories"][category]["total_bytes"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
        }
    }

    #[test]
    fn should_report_local_capacity_bytes_by_category() {
        // Arrange
        use_local_storage();
        let path = data_dir("category-bytes");
        let path_for_cleanup = path.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_capacity_fixture(&cassie, &session);

            // Act
            let metrics = cassie.metrics();
            let capacity = &metrics["capacity"];

            // Assert
            assert_capacity_metadata(capacity);
            assert_capacity_families(capacity);
            assert_capacity_categories(capacity);
        });

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }
}

// Formerly tests/metrics_feedback.rs.
mod metrics_feedback {
    #![allow(unused_imports, dead_code)]

    use super::support_pgwire as pgwire_support;

    use cassie::app::{Cassie, CassieSession};
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::midge::adapter::set_operator_feedback_persistence_failure_point;
    use cassie::midge::StorageFamily;
    use cassie::runtime::{RuntimeFeedbackKey, RuntimeFeedbackObservation, RuntimeFeedbackRecord};
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{data_dir, describe_statement_frame, startup_frame, use_local_storage};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn operator_feedback_config(enabled: bool) -> cassie::config::CassieRuntimeConfig {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.operator_feedback_enabled = enabled;
        config
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
                options: BTreeMap::default(),
            };
            cassie.midge.put_index(&index).unwrap();
            cassie.catalog.register_index(index);
        }
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
        pgwire_support::read_wire_frame(reader).await
    }

    #[test]
    fn should_isolate_runtime_feedback_by_offset_shape() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_offset_shape");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_offset_shape";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let without_offset_sql =
                "SELECT title FROM metrics_feedback_offset_shape ORDER BY title LIMIT 1";
            let with_offset_sql =
                "SELECT title FROM metrics_feedback_offset_shape ORDER BY title LIMIT 1 OFFSET 500";
            let without_offset_key = feedback_key(&cassie, &session, without_offset_sql, None);
            let with_offset_key = feedback_key(&cassie, &session, with_offset_sql, None);

            // Act
            cassie
                .execute_sql(&session, without_offset_sql, vec![])
                .expect("execute query without OFFSET");
            cassie
                .execute_sql(&session, with_offset_sql, vec![])
                .expect("execute query with OFFSET");
            let without_offset_record = cassie
                .feedback_record_for_diagnostics(&without_offset_key)
                .expect("feedback without OFFSET");
            let with_offset_record = cassie
                .feedback_record_for_diagnostics(&with_offset_key)
                .expect("feedback with OFFSET");

            // Assert
            assert_ne!(
                without_offset_key.predicate_shape_hash,
                with_offset_key.predicate_shape_hash
            );
            assert_eq!(without_offset_record.executions, 1);
            assert_eq!(with_offset_record.executions, 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_capture_runtime_feedback_for_normalized_select() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_capture");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_capture";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_feedback_capture WHERE title = $1";
            let key = feedback_key(&cassie, &session, sql, None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            let metrics = cassie.metrics();
            let record = cassie
                .feedback_record_for_diagnostics(&key)
                .expect("scan feedback should be recorded");

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(record.executions, 1);
            assert_eq!(record.rows_out_total, 1);
            assert_eq!(record.errors_total, 0);
            assert!(
                metrics["feedback"]["writes"].as_u64().unwrap_or_default() >= 1,
                "feedback writes should be tracked"
            );
            assert!(
                metrics["feedback"]["misses"].as_u64().unwrap_or_default() >= 1,
                "first feedback lookup should miss"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_capture_runtime_feedback_for_selected_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_selected_index");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_selected_index";
            let index_name = "metrics_feedback_selected_title_idx";
            register_feedback_collection(&cassie, collection);
            let index = IndexMeta {
                collection: collection.to_string(),
                name: index_name.to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: false,
                options: BTreeMap::default(),
            };
            cassie.midge.put_index(&index).unwrap();
            cassie.catalog.register_index(index);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT body FROM metrics_feedback_selected_index WHERE title = $1";
            let key = feedback_key(&cassie, &session, sql, Some(index_name));

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            let explained = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT body FROM metrics_feedback_selected_index WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let record = cassie
                .feedback_record_for_diagnostics(&key)
                .expect("selected index feedback should be recorded");

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(record.executions, 1);
            assert_eq!(record.rows_out_total, 1);
            assert_eq!(key.operator_family, "index_read");
            let cassie::types::Value::String(plan) = &explained.rows[0][0] else {
                panic!("expected explain string");
            };
            assert!(plan.contains("index_feedback=enabled"));
            assert!(plan.contains(index_name));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_skip_feedback_lookup_when_execution_result_cache_hits() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_result_cache_hit");
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
                    "CREATE TABLE feedback_result_cache_hit (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO feedback_result_cache_hit (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM feedback_result_cache_hit WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let after_first = cassie.metrics();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM feedback_result_cache_hit WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let after_second = cassie.metrics();

            // Assert
            assert_eq!(first.rows, second.rows);
            assert_eq!(
                after_second["feedback"]["hits"].as_u64(),
                after_first["feedback"]["hits"].as_u64()
            );
            assert_eq!(
                after_second["feedback"]["misses"].as_u64(),
                after_first["feedback"]["misses"].as_u64()
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_aggregate_runtime_feedback_across_parameter_values() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_aggregate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_aggregate";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_feedback_aggregate WHERE title = $1";
            let key = feedback_key(&cassie, &session, sql, None);

            // Act
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("beta".to_string())],
                )
                .unwrap();
            let metrics = cassie.metrics();
            let record = cassie
                .feedback_record_for_diagnostics(&key)
                .expect("scan feedback should aggregate");

            // Assert
            assert_eq!(record.executions, 2);
            assert_eq!(record.rows_out_total, 2);
            assert!(
                metrics["feedback"]["hits"].as_u64().unwrap_or_default() >= 1,
                "second feedback lookup should hit"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_persist_runtime_feedback_delta_without_replacing_unrelated_records() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_delta_persistence");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_delta_persistence";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_feedback_delta_persistence WHERE title = $1";
            let key = feedback_key(&cassie, &session, sql, None);
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            let current = cassie
                .feedback_record_for_diagnostics(&key)
                .expect("current feedback record");
            let unrelated_key = RuntimeFeedbackKey {
                collection: "unrelated_persisted_feedback".to_string(),
                predicate_shape_hash: key.predicate_shape_hash.wrapping_add(1),
                ..key.clone()
            };
            let unrelated_record = RuntimeFeedbackRecord {
                executions: 17,
                first_seen_ms: 1,
                last_seen_ms: 2,
                ..RuntimeFeedbackRecord::default()
            };
            cassie
                .midge
                .replace_runtime_feedback_records(&[
                    (key.clone(), current),
                    (unrelated_key.clone(), unrelated_record),
                ])
                .expect("seed persisted feedback records");

            // Act
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("beta".to_string())],
                )
                .unwrap();
            let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

            // Assert
            let unrelated = persisted
                .iter()
                .find(|(persisted_key, _)| persisted_key == &unrelated_key)
                .map(|(_, record)| record)
                .expect("unrelated persisted feedback must remain untouched");
            assert_eq!(unrelated.executions, 17);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reconcile_runtime_feedback_after_delta_persistence_failure() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_delta_retry");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_delta_retry";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let first_sql = "SELECT title FROM metrics_feedback_delta_retry";
            let second_sql = "SELECT body FROM metrics_feedback_delta_retry";
            let first_key = feedback_key(&cassie, &session, first_sql, None);
            let second_key = feedback_key(&cassie, &session, second_sql, None);
            let before_errors = cassie.metrics()["storage"]["schema"]["errors"]
                .as_u64()
                .unwrap_or_default();
            set_operator_feedback_persistence_failure_point(true);

            // Act
            cassie.execute_sql(&session, first_sql, vec![]).unwrap();
            let after_failure = cassie.midge.list_runtime_feedback_records().unwrap();
            cassie.execute_sql(&session, second_sql, vec![]).unwrap();
            let after_reconciliation = cassie.midge.list_runtime_feedback_records().unwrap();
            let after_errors = cassie.metrics()["storage"]["schema"]["errors"]
                .as_u64()
                .unwrap_or_default();

            // Assert
            assert!(cassie.feedback_record_for_diagnostics(&first_key).is_some());
            assert!(
                after_failure
                    .iter()
                    .all(|(persisted_key, _)| persisted_key != &first_key),
                "failed delta must not partially commit: {after_failure:?}"
            );
            assert_eq!(after_errors, before_errors.saturating_add(1));
            assert!(
                [&first_key, &second_key].iter().all(|expected| {
                    after_reconciliation
                        .iter()
                        .any(|(persisted_key, _)| persisted_key == *expected)
                }),
                "reconciled={after_reconciliation:?}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_ignore_malformed_persisted_feedback_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_malformed_restart");
        let config = operator_feedback_config(true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE metrics_feedback_malformed_restart (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO metrics_feedback_malformed_restart (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM metrics_feedback_malformed_restart WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let feedback_storage_key = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Schema, b"")
                .unwrap()
                .into_iter()
                .find_map(|(key, value)| {
                    serde_json::from_slice::<serde_json::Value>(&value)
                        .ok()
                        .filter(|record| record.get("record").is_some() && record.get("key").is_some())
                        .map(|_| key)
                })
                .expect("persisted runtime feedback entry");
            cassie
                .midge
                .raw_put(StorageFamily::Schema, &feedback_storage_key, b"malformed-feedback")
                .unwrap();
            drop(session);
            drop(cassie);

            // Act
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            let plan = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM metrics_feedback_malformed_restart WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap()
                .rows[0][0]
                .as_str()
                .unwrap()
                .to_string();

            // Assert
            assert!(cassie.midge.list_runtime_feedback_records().unwrap().is_empty());
            assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
            assert!(
                plan.contains("operator_feedback_reason=missing"),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_partition_runtime_feedback_by_schema_epoch() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_schema_epoch");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_schema_epoch";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_feedback_schema_epoch WHERE title = $1";
            let first_key = feedback_key(&cassie, &session, sql, None);

            // Act
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE feedback_schema_marker (id INT)",
                    vec![],
                )
                .unwrap();
            let second_key = feedback_key(&cassie, &session, sql, None);
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("beta".to_string())],
                )
                .unwrap();
            let first = cassie.feedback_record_for_diagnostics(&first_key).is_none();
            let second = cassie
                .feedback_record_for_diagnostics(&second_key)
                .expect("second schema epoch feedback");
            let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

            // Assert
            assert!(first, "schema changes should invalidate older feedback");
            assert_eq!(second.executions, 1);
            assert!(
                persisted
                    .iter()
                    .all(|(key, _)| key.schema_epoch == second_key.schema_epoch),
                "persisted feedback must not retain an invalid schema epoch: {persisted:?}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_feedback_record_from_stale_schema_epoch() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_stale_schema_epoch");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_stale_schema_epoch";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_feedback_stale_schema_epoch";
            let stale_key = feedback_key(&cassie, &session, sql, None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE feedback_new_schema_epoch (id INT)",
                    vec![],
                )
                .unwrap();

            // Act
            cassie
                .seed_feedback_for_diagnostics(&stale_key, &confident_feedback(5, 1))
                .expect("discard stale-epoch feedback");
            let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

            // Assert
            assert!(cassie.feedback_record_for_diagnostics(&stale_key).is_none());
            assert!(
                persisted
                    .iter()
                    .all(|(persisted_key, _)| persisted_key != &stale_key),
                "persisted={persisted:?}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_evict_runtime_feedback_when_retention_limit_is_exceeded() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_eviction");
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.feedback_entries = 1;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let collection = "metrics_feedback_eviction";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let first_sql = "SELECT title FROM metrics_feedback_eviction";
            let second_sql = "SELECT body FROM metrics_feedback_eviction";
            let first_key = feedback_key(&cassie, &session, first_sql, None);

            // Act
            cassie.execute_sql(&session, first_sql, vec![]).unwrap();
            cassie.execute_sql(&session, second_sql, vec![]).unwrap();
            let metrics = cassie.metrics();
            let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

            // Assert
            assert!(cassie.feedback_record_for_diagnostics(&first_key).is_none());
            assert_eq!(metrics["feedback"]["entries"].as_u64(), Some(1));
            assert_eq!(persisted.len(), 1, "persisted={persisted:?}");
            assert_ne!(persisted[0].0, first_key);
            assert!(
                metrics["feedback"]["evictions"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1,
                "retention limit should evict the oldest feedback"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_ignore_operator_feedback_when_confidence_is_too_low() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_feedback_low_confidence");
        let config = operator_feedback_config(true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let collection = "metrics_operator_feedback_low_confidence";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_operator_feedback_low_confidence WHERE title = $1";
            let key = feedback_key(&cassie, &session, sql, None);

            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();

            // Act
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM metrics_operator_feedback_low_confidence WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let plan = explain.rows[0][0].as_str().unwrap().to_string();
            let record = cassie
                .feedback_record_for_diagnostics(&key)
                .expect("low-confidence feedback");

            // Assert
            assert!(record.confidence_bps < 600, "record={record:?}");
            assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
            assert!(
                plan.contains("operator_feedback_reason=low_confidence"),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_use_confident_operator_feedback_to_switch_selected_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_feedback_switch");
        let config = operator_feedback_config(true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let collection = "metrics_operator_feedback_switch";
            let base_index = "metrics_operator_feedback_body_idx_a";
            let preferred_index = "metrics_operator_feedback_title_idx_b";
            register_feedback_collection(&cassie, collection);
            register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
            let session = cassie.create_session("tester", None);
            let explain_sql = "EXPLAIN SELECT title FROM metrics_operator_feedback_switch WHERE title = 'alpha' AND body = 'one'";
            let shape_sql = "SELECT title FROM metrics_operator_feedback_switch WHERE title = 'alpha' AND body = 'one'";
            let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
            let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));

            let baseline = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
            let baseline_plan = baseline.rows[0][0].as_str().unwrap().to_string();
            assert!(baseline_plan.contains(base_index), "plan={baseline_plan}");

            for _ in 0..4 {
                cassie
                    .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                    .expect("seed base feedback");
                cassie
                    .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                    .expect("seed preferred feedback");
            }
            let base_record = cassie
                .feedback_record_for_diagnostics(&base_key)
                .expect("base record");
            let preferred_record = cassie
                .feedback_record_for_diagnostics(&preferred_key)
                .expect("preferred record");
            assert_ne!(base_key, preferred_key);
            assert!(
                preferred_record.stable_average_elapsed_ms()
                    < base_record.stable_average_elapsed_ms()
            );

            // Act
            let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
            let plan = explain.rows[0][0].as_str().unwrap().to_string();

            // Assert
            assert!(plan.contains(preferred_index), "plan={plan}");
            assert!(plan.contains("operator_feedback=used"), "plan={plan}");
            assert!(
                plan.contains(&format!(
                    "operator_feedback_base_candidate=index:{base_index}"
                )),
                "plan={plan}"
            );
            assert!(
                plan.contains(&format!(
                    "operator_feedback_selected_candidate=index:{preferred_index}"
                )),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_fall_back_to_base_index_when_operator_feedback_is_disabled() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_feedback_disabled");
        let config = operator_feedback_config(false);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let collection = "metrics_operator_feedback_disabled";
            let base_index = "metrics_operator_feedback_disabled_body_idx_a";
            let preferred_index = "metrics_operator_feedback_disabled_title_idx_b";
            register_feedback_collection(&cassie, collection);
            register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
            let session = cassie.create_session("tester", None);
            let explain_sql = "EXPLAIN SELECT title FROM metrics_operator_feedback_disabled WHERE title = 'alpha' AND body = 'one'";
            let shape_sql = "SELECT title FROM metrics_operator_feedback_disabled WHERE title = 'alpha' AND body = 'one'";
            let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
            let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));

            for _ in 0..4 {
                cassie
                    .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                    .expect("seed base feedback");
                cassie
                    .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                    .expect("seed preferred feedback");
            }

            // Act
            let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
            let plan = explain.rows[0][0].as_str().unwrap().to_string();

            // Assert
            assert!(plan.contains(base_index), "plan={plan}");
            assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
            assert!(
                plan.contains("operator_feedback_reason=disabled"),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_stale_operator_feedback_in_explain_diagnostics() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_feedback_stale");
        let mut config = operator_feedback_config(true);
        config.limits.feedback_ttl_seconds = 1;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let collection = "metrics_operator_feedback_stale";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_operator_feedback_stale WHERE title = $1";

            for value in ["alpha", "beta", "alpha", "beta"] {
                cassie
                    .execute_sql(
                        &session,
                        sql,
                        vec![cassie::types::Value::String(value.to_string())],
                    )
                    .unwrap();
            }
            std::thread::sleep(Duration::from_millis(1_200));

            // Act
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM metrics_operator_feedback_stale WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let plan = explain.rows[0][0].as_str().unwrap().to_string();

            // Assert
            assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
            assert!(
                plan.contains("operator_feedback_reason=stale"),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_persisted_operator_feedback_from_storage() {
        // Arrange
        use_local_storage();
        let path = data_dir("operator_feedback_restart");
        let config = operator_feedback_config(true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE metrics_operator_feedback_restart (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            for (title, body) in [("alpha", "one"), ("beta", "two"), ("gamma", "three")] {
                cassie
                    .execute_sql(
                        &session,
                        "INSERT INTO metrics_operator_feedback_restart (title, body) VALUES ($1, $2)",
                        vec![
                            cassie::types::Value::String(title.to_string()),
                            cassie::types::Value::String(body.to_string()),
                        ],
                    )
                    .unwrap();
            }
            let sql = "SELECT title FROM metrics_operator_feedback_restart WHERE title = $1";
            let key = feedback_key(&cassie, &session, sql, None);

            for value in ["alpha", "beta", "gamma"] {
                cassie
                    .execute_sql(
                        &session,
                        sql,
                        vec![cassie::types::Value::String(value.to_string())],
                    )
                    .unwrap();
            }
            assert!(
                !cassie
                    .midge
                    .list_runtime_feedback_records()
                    .unwrap()
                    .is_empty(),
                "feedback records should be persisted into storage"
            );
            cassie.clear_feedback_for_diagnostics();
            assert!(
                cassie.feedback_record_for_diagnostics(&key).is_none(),
                "clearing runtime feedback should remove the in-memory record"
            );

            cassie
                .reload_feedback_from_storage_for_diagnostics()
                .expect("reload feedback from storage");

            // Act
            let record = cassie
                .feedback_record_for_diagnostics(&key)
                .expect("persisted feedback record");

            // Assert
            assert_eq!(record.executions, 3, "record={record:?}");
            assert!(record.confidence_bps >= 600, "record={record:?}");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_runtime_feedback_in_explain_analyze_output() {
        // Arrange
        use_local_storage();
        let path = data_dir("feedback_explain_analyze");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_feedback_explain_analyze";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);

            // Act
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN ANALYZE SELECT title FROM metrics_feedback_explain_analyze WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let plan = explain.rows[0][0].as_str().unwrap().to_string();
            let metrics = cassie.metrics();

            // Assert
            assert!(plan.contains("analyze=true"), "plan={plan}");
            assert!(plan.contains("operator_actuals=Scan:"), "plan={plan}");
            assert!(plan.contains("rows_out:1"), "plan={plan}");
            assert!(
                metrics["feedback"]["writes"].as_u64().unwrap_or_default() >= 1,
                "EXPLAIN ANALYZE should write feedback"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_use_runtime_feedback_for_candidate_budget() {
        // Arrange
        use_local_storage();
        let path = data_dir("adaptive_candidate_feedback");
        let config = adaptive_candidate_config(1, 100);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let collection = "metrics_adaptive_candidate_feedback";
            register_adaptive_candidate_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_feedback ORDER BY score DESC LIMIT 1";
            let wider_sql = "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_feedback ORDER BY score DESC LIMIT 2";

            cassie.execute_sql(&session, sql, vec![]).unwrap();
            let seeded = cassie.metrics();

            // Act
            cassie.execute_sql(&session, wider_sql, vec![]).unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                after["adaptive_candidates"]["initial_budget_total"]
                    .as_u64()
                    .unwrap_or_default()
                    - seeded["adaptive_candidates"]["initial_budget_total"]
                        .as_u64()
                        .unwrap_or_default(),
                3
            );
            assert_eq!(
                after["adaptive_candidates"]["feedback_budget_total"]
                    .as_u64()
                    .unwrap_or_default()
                    - seeded["adaptive_candidates"]["feedback_budget_total"]
                        .as_u64()
                        .unwrap_or_default(),
                3
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/metrics_joins.rs.
mod metrics_joins {
    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::Value;

    use super::support_metrics as support;
    use support::*;

    fn vectorized_join_config(batch_size: usize) -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.vectorized_joins_enabled = true;
        config.limits.vectorized_join_batch_size = batch_size;
        config
    }

    #[test]
    fn should_record_merge_join_runtime_metrics() {
        // Arrange
        use_local_storage();
        let path = data_dir("merge_join_runtime_metrics");
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
                "CREATE TABLE metrics_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_join_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_join_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_join_users.name, metrics_join_orders.total FROM metrics_join_users JOIN metrics_join_orders ON metrics_join_users.user_key = metrics_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("ada".to_string()), Value::Int64(42)]]
        );
        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["executions"], 1);
        assert_eq!(metrics["joins"]["merge_joins"], 1);
        assert_eq!(metrics["joins"]["matched_rows_total"], 1);
        assert_eq!(metrics["joins"]["output_rows_total"], 1);
        assert_eq!(metrics["joins"]["last_strategy"], "merge");

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_record_vectorized_join_runtime_metrics() {
        // Arrange
        use_local_storage();
        let path = data_dir("vectorized_join_runtime_metrics");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config(1)).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_vector_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_vector_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO metrics_vector_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO metrics_vector_users (user_key, name) VALUES (2, 'grace')",
            "INSERT INTO metrics_vector_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO metrics_vector_orders (order_user_key, total) VALUES (2, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_vector_users.name, metrics_vector_orders.total FROM metrics_vector_users JOIN metrics_vector_orders ON metrics_vector_users.user_key = metrics_vector_orders.order_user_key ORDER BY metrics_vector_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Int64(7)]
            ]
        );
        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["vectorized_joins"], 1);
        assert_eq!(metrics["joins"]["vectorized_batches_total"], 2);
        assert_eq!(metrics["joins"]["vectorized_build_rows_total"], 2);
        assert_eq!(metrics["joins"]["vectorized_probe_rows_total"], 2);
        assert_eq!(metrics["joins"]["last_strategy"], "vectorized");

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_record_vectorized_join_spill_fallback() {
        // Arrange
        use_local_storage();
        let path = data_dir("vectorized_join_spill_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let mut config = vectorized_join_config(2);
        config.limits.query_memory_budget_bytes = 800;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_spill_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_spill_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_spill_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_spill_orders (order_user_key, total) VALUES (2, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_spill_users.name, metrics_spill_orders.total FROM metrics_spill_users JOIN metrics_spill_orders ON metrics_spill_users.user_key = metrics_spill_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());
        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["vectorized_joins"], 0);
        assert_eq!(metrics["joins"]["vectorized_fallbacks"], 1);
        assert_eq!(metrics["joins"]["vectorized_spill_fallbacks"], 1);
        assert_eq!(
            metrics["joins"]["last_vectorized_fallback_reason"],
            "spill_budget_exceeded"
        );
        assert_eq!(metrics["joins"]["last_strategy"], "merge");

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/metrics_plan_pgwire.rs.
mod metrics_plan_pgwire {
    use super::support_pgwire as pgwire_support;

    use cassie::app::Cassie;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{data_dir, describe_statement_frame, startup_frame, use_local_storage};

    fn password_frame(password: &str) -> Vec<u8> {
        pgwire_support::password_message(password)
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
        pgwire_support::read_wire_frame(reader).await
    }

    async fn read_until_ready(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> Vec<u8> {
        pgwire_support::read_until_ready(reader).await
    }

    #[test]
    fn should_report_plan_cache_metrics() {
        // Arrange
        use_local_storage();
        let path = data_dir("plan_cache_metrics");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "metrics_plan_cache_docs";
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
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("tester", None);

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM metrics_plan_cache_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM metrics_plan_cache_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));
            assert!(
                metrics["plan_cache"]["entries"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_track_protocol_errors_for_missing_prepared_statement_describe() {
        // Arrange
        use_local_storage();
        let path = data_dir("pgwire_protocol_errors");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config =
                cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let before_protocol_errors = cassie.metrics()["pgwire"]["protocol_errors_total"]
                .as_u64()
                .unwrap_or_default();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                std::sync::Arc::new(cassie.clone()),
                config,
            ));

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            let startup = startup_frame("root", "postgres");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
                .await
                .expect("startup write");

            let auth_frame = read_auth_frame(&mut reader).await;
            assert_eq!(
                auth_frame.0, b'R',
                "startup should return an authentication response"
            );
            let auth_code = i32::from_be_bytes(
                auth_frame.2[..4]
                    .try_into()
                    .expect("authentication request code"),
            );
            assert_eq!(
                auth_code, 3,
                "configured listener should request a password"
            );
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &password_frame("postgres"))
                .await
                .expect("password write");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("password flush");
            let auth_ok = read_auth_frame(&mut reader).await;
            assert_eq!(auth_ok.0, b'R', "password should return auth response");
            assert_eq!(
                i32::from_be_bytes(auth_ok.2[..4].try_into().expect("authentication ok code"),),
                0,
                "password authentication should complete"
            );
            let startup_ready = read_until_ready(&mut reader).await;
            assert_eq!(startup_ready, vec![b'I']);

            // Act
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &describe_statement_frame("missing"),
            )
            .await
            .expect("describe write");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush");
            let response = read_wire_frame(&mut reader).await;
            assert_eq!(response.0, b'E', "describe should return an error frame");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            drop(socket);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let metrics = cassie.metrics();

            // Assert
            assert_eq!(
                metrics["pgwire"]["protocol_errors_total"]
                    .as_u64()
                    .unwrap_or_default()
                    - before_protocol_errors,
                1,
                "missing describe statement should count as a protocol error"
            );

            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/metrics_read_paths.rs.
mod metrics_read_paths {
    #![allow(unused_imports, dead_code)]

    use cassie::app::{Cassie, CassieSession};
    use cassie::types::{DataType, FieldSchema, Schema};
    use serde_json::Value as JsonValue;

    use super::support_sql as support;
    use support::*;

    fn ordered_read_schema() -> Schema {
        Schema {
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
        }
    }

    fn scalar_read_schema() -> Schema {
        Schema {
            fields: vec![
                FieldSchema {
                    name: "tenant_id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        }
    }

    fn register_collection_with_schema(cassie: &Cassie, collection: &str, schema: Schema) {
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(collection, schema);
    }

    fn insert_documents(cassie: &Cassie, collection: &str, docs: Vec<(Option<String>, JsonValue)>) {
        for (id, payload) in docs {
            cassie.midge.put_document(collection, id, payload).unwrap();
        }
    }

    fn seed_ordered_read_collection(cassie: &Cassie, collection: &str) {
        register_collection_with_schema(cassie, collection, ordered_read_schema());
        insert_documents(
            cassie,
            collection,
            vec![
                (
                    Some("d1".to_string()),
                    serde_json::json!({"title": "one", "score": 1}),
                ),
                (
                    Some("d2".to_string()),
                    serde_json::json!({"title": "two", "score": 2}),
                ),
                (
                    Some("d3".to_string()),
                    serde_json::json!({"title": "three", "score": 3}),
                ),
            ],
        );
    }

    fn seed_scalar_read_collection(cassie: &Cassie, collection: &str) {
        register_collection_with_schema(cassie, collection, scalar_read_schema());
        let bootstrap = cassie.create_session("bootstrap", None);
        for sql in [
        "CREATE INDEX metrics_scalar_title_idx ON metrics_scalar_read_paths USING btree (title)",
        "CREATE INDEX metrics_scalar_tenant_status_idx ON metrics_scalar_read_paths USING btree (tenant_id, status)",
    ] {
        cassie.execute_sql(&bootstrap, sql, vec![]).unwrap();
    }
        insert_documents(
            cassie,
            collection,
            vec![
                (
                    None,
                    serde_json::json!({
                        "tenant_id": "tenant-a",
                        "status": "closed",
                        "title": "alpha",
                    }),
                ),
                (
                    None,
                    serde_json::json!({
                        "tenant_id": "tenant-a",
                        "status": "open",
                        "title": "beta",
                    }),
                ),
                (
                    None,
                    serde_json::json!({
                        "tenant_id": "tenant-b",
                        "status": "closed",
                        "title": "charlie",
                    }),
                ),
                (
                    None,
                    serde_json::json!({
                        "tenant_id": "tenant-b",
                        "status": "open",
                        "title": "delta",
                    }),
                ),
            ],
        );
    }

    fn execute_queries(cassie: &Cassie, session: &CassieSession, queries: &[&str]) {
        for sql in queries {
            cassie.execute_sql(session, sql, vec![]).unwrap();
        }
    }

    fn assert_ordered_read_metrics(before: &serde_json::Value, after: &serde_json::Value) {
        assert_eq!(
            after["read_paths"]["ordered_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["ordered_scans"]
                .as_u64()
                .unwrap_or_default()
                + 4,
        );
        assert_eq!(
            after["read_paths"]["ordered_rows"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["ordered_rows"]
                .as_u64()
                .unwrap_or_default()
                + 5,
        );
        assert_eq!(
            after["read_paths"]["storage_top_k_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["storage_top_k_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["keyset_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["keyset_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["degraded_offset_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["degraded_offset_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["heap_top_k_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["heap_top_k_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["last_ordered_scan_mode"].as_str(),
            Some("heap_top_k"),
        );
    }

    fn assert_scalar_read_metrics(before: &serde_json::Value, after: &serde_json::Value) {
        assert_eq!(
            after["read_paths"]["index_seek_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["index_seek_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["prefix_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["prefix_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["range_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["range_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["ordered_bounded_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["ordered_bounded_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_mode"].as_str(),
            Some("ordered_bounded_scan"),
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_index"].as_str(),
            Some("metrics_scalar_title_idx"),
        );
    }

    #[test]
    fn should_record_runtime_metrics_for_ordered_read_paths() {
        // Arrange
        use_local_storage();
        let path = data_dir("metrics_read_paths_ordered");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            seed_ordered_read_collection(&cassie, "metrics_ordered_read_paths");
            let session = cassie.create_session("tester", None);
            let before = cassie.metrics();

            // Act
            execute_queries(
            &cassie,
            &session,
            &[
                "SELECT id FROM metrics_ordered_read_paths ORDER BY id ASC LIMIT 2",
                "SELECT id FROM metrics_ordered_read_paths WHERE id > 'd1' ORDER BY id ASC LIMIT 1",
                "SELECT id FROM metrics_ordered_read_paths ORDER BY id ASC LIMIT 1 OFFSET 1",
                "SELECT id FROM metrics_ordered_read_paths ORDER BY score DESC LIMIT 1",
            ],
        );

            // Assert
            let after = cassie.metrics();
            assert_ordered_read_metrics(&before, &after);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_record_runtime_metrics_for_scalar_index_read_paths() {
        // Arrange
        use_local_storage();
        let path = data_dir("metrics_read_paths_scalar");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        seed_scalar_read_collection(&cassie, "metrics_scalar_read_paths");
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        execute_queries(
            &cassie,
            &session,
            &[
                "SELECT title FROM metrics_scalar_read_paths WHERE title = 'alpha'",
                "SELECT title FROM metrics_scalar_read_paths WHERE tenant_id = 'tenant-a' AND status = 'open'",
                "SELECT title FROM metrics_scalar_read_paths WHERE title >= 'beta' AND title < 'omega' ORDER BY title ASC",
                "SELECT title FROM metrics_scalar_read_paths ORDER BY title ASC LIMIT 2",
            ],
        );

        // Assert
        let after = cassie.metrics();
        assert_scalar_read_metrics(&before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/metrics_runtime.rs.
mod metrics_runtime {
    use cassie::app::Cassie;
    use cassie::catalog::{canonical_relation_name, IndexKind, IndexMeta};
    use cassie::types::{DataType, FieldSchema, Schema};

    use super::support_metrics as support;
    use support::{data_dir, use_local_storage};

    fn canonical_public_relation(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    struct ReadPathBaseline {
        point_hits: u64,
        point_misses: u64,
        point_scans: u64,
        collection_scans: u64,
    }

    fn seed_read_path_collection(cassie: &Cassie) {
        let collection = "metrics_read_paths";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
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
        cassie.register_collection(collection, schema);
        for (id, payload) in [
            (
                "doc-1",
                serde_json::json!({"title": "alpha", "status": "active"}),
            ),
            (
                "doc-2",
                serde_json::json!({"title": "bravo", "status": "queued"}),
            ),
        ] {
            cassie
                .midge
                .put_document(collection, Some(id.to_string()), payload)
                .unwrap();
        }
    }

    fn seed_cardinality_metrics_fixture(cassie: &Cassie, collection: &str, index: &str) {
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

        cassie.midge.create_database("postgres", None).unwrap();
        cassie.midge.create_namespace("postgres.public").unwrap();
        cassie
            .midge
            .create_collection(collection, schema.clone())
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
                name: index.to_string(),
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
        cassie.midge.delete_cardinality_stats(collection).unwrap();
    }

    fn read_path_baseline(metrics: &serde_json::Value) -> ReadPathBaseline {
        ReadPathBaseline {
            point_hits: metrics["read_paths"]["point_lookup_hits"]
                .as_u64()
                .unwrap_or_default(),
            point_misses: metrics["read_paths"]["point_lookup_misses"]
                .as_u64()
                .unwrap_or_default(),
            point_scans: metrics["read_paths"]["point_lookup_scans"]
                .as_u64()
                .unwrap_or_default(),
            collection_scans: metrics["read_paths"]["collection_scans"]
                .as_u64()
                .unwrap_or_default(),
        }
    }

    fn execute_read_path_queries(cassie: &Cassie, session: &cassie::app::CassieSession) {
        for sql in [
            "SELECT title FROM metrics_read_paths WHERE id = 'doc-1'",
            "SELECT title FROM metrics_read_paths WHERE id = 'missing'",
            "SELECT title FROM metrics_read_paths",
        ] {
            cassie.execute_sql(session, sql, vec![]).unwrap();
        }
    }

    fn assert_read_path_metrics(after: &serde_json::Value, before: &ReadPathBaseline) {
        assert_eq!(
            after["read_paths"]["point_lookup_scans"]
                .as_u64()
                .unwrap_or_default(),
            before.point_scans + 2,
        );
        assert_eq!(
            after["read_paths"]["point_lookup_hits"]
                .as_u64()
                .unwrap_or_default(),
            before.point_hits + 1,
        );
        assert_eq!(
            after["read_paths"]["point_lookup_misses"]
                .as_u64()
                .unwrap_or_default(),
            before.point_misses + 1,
        );
        assert_eq!(
            after["read_paths"]["collection_scans"]
                .as_u64()
                .unwrap_or_default(),
            before.collection_scans + 1,
        );
        assert!(after["read_paths"]["last_point_lookup_collection"]
            .as_str()
            .is_some());
    }

    #[test]
    fn should_report_runtime_metrics_snapshot() {
        // Arrange
        use_local_storage();
        let path = data_dir("startup_query");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = "metrics_runtime_docs";
            let canonical_collection = canonical_public_relation(collection);
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(&canonical_collection, schema.clone())
                .unwrap();
            cassie.register_collection(&canonical_collection, schema.clone());
            cassie
                .midge
                .put_document(
                    &canonical_collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM metrics_runtime_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM metrics_runtime_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(metrics["ready"], serde_json::Value::Bool(true));
            assert!(
                metrics["runtime"]["startup_total"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1,
                "startup counter should be recorded"
            );
            assert!(
                metrics["runtime"]["catalog_hydration_total"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1,
                "catalog hydration counter should be recorded"
            );
            assert_eq!(metrics["query"]["count"].as_u64(), Some(2));
            assert_eq!(metrics["query"]["rows_returned_total"].as_u64(), Some(2));
            assert!(
                metrics["storage"]["schema"]["reads"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0,
                "schema storage reads should be recorded"
            );
            assert!(
                metrics["storage"]["data"]["reads"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0,
                "data storage reads should be recorded"
            );
            assert!(
                metrics["storage"]["temp"]["writes"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0,
                "temp storage writes should be recorded"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_record_read_path_metrics_for_point_lookup_collection_scan() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_path_metrics");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            seed_read_path_collection(&cassie);
            let session = cassie.create_session("tester", None);
            let before = read_path_baseline(&cassie.metrics());

            // Act
            execute_read_path_queries(&cassie, &session);

            // Assert
            let after = cassie.metrics();
            assert_read_path_metrics(&after, &before);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_expose_cardinality_metrics_with_explain_plan_estimates() {
        // Arrange
        use_local_storage();
        let path = data_dir("cardinality_metrics");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = canonical_public_relation("metrics_cardinality_docs");
            let index = canonical_public_relation("idx_title");
            seed_cardinality_metrics_fixture(&cassie, &collection, &index);

            // Act
            cassie.startup().unwrap();
            cassie
                .ingest_document(
                    &collection,
                    serde_json::json!({"title": "beta", "body": "charlie"}),
                )
                .unwrap();

            let session = cassie.create_session("tester", None);
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM metrics_cardinality_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let plan = explain.rows[0][0].as_str().unwrap().to_string();
            let metrics = cassie.metrics();

            // Assert
            assert!(plan.contains("estimates=scan:2"), "plan={plan}");
            assert!(plan.contains("index:1"), "plan={plan}");
            assert!(plan.contains("cost_source=advanced_stats"), "plan={plan}");
            assert!(
                metrics["cardinality"]["reads"].as_u64().unwrap_or_default() >= 1,
                "cardinality reads should be tracked"
            );
            assert!(
                metrics["cardinality"]["writes"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1,
                "cardinality writes should be tracked"
            );
            assert!(
                metrics["cardinality"]["rebuilds"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1,
                "cardinality rebuilds should be tracked"
            );
            assert!(
                metrics["cardinality"]["unavailable"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1,
                "missing stats should be tracked"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_record_query_error_statistics() {
        // Arrange
        use_local_storage();
        let path = data_dir("query_errors");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            let before = cassie.metrics();
            let before_count = before["query"]["count"].as_u64().unwrap_or_default();
            let before_errors = before["query"]["errors_total"].as_u64().unwrap_or_default();

            // Act
            let result = cassie.execute_sql(
                &session,
                "SELECT title FROM metrics_missing_query_errors",
                vec![],
            );
            let after = cassie.metrics();

            // Assert
            assert!(result.is_err(), "missing collection should fail");
            assert_eq!(
                after["query"]["count"].as_u64().unwrap_or_default() - before_count,
                1
            );
            assert_eq!(
                after["query"]["errors_total"].as_u64().unwrap_or_default() - before_errors,
                1
            );
            assert!(after["query"]["errors_by_class"]
                .as_object()
                .expect("errors by class")
                .values()
                .any(|count| count.as_u64().unwrap_or_default() > 0));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_count_failed_scan_as_storage_read_error() {
        // Arrange
        use_local_storage();
        let path = data_dir("scan_errors");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.catalog.register_collection(
                "missing_storage_collection",
                vec![("title".to_string(), DataType::Text)],
            );

            let before = cassie.metrics();
            let before_errors = before["storage"]["data"]["errors"]
                .as_u64()
                .unwrap_or_default();
            let before_reads = before["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default();

            let session = cassie.create_session("tester", None);
            // Act
            let result = cassie.execute_sql(
                &session,
                "SELECT title FROM missing_storage_collection WHERE title = 'alpha'",
                vec![],
            );
            assert!(
                result.is_err(),
                "query should fail because collection schema is missing in storage"
            );

            let after = cassie.metrics();

            // Assert
            assert_eq!(
                after["storage"]["data"]["errors"]
                    .as_u64()
                    .unwrap_or_default()
                    - before_errors,
                1
            );
            assert!(
                after["storage"]["data"]["reads"]
                    .as_u64()
                    .unwrap_or_default()
                    > before_reads,
                "scan failure should still record the read attempt"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/metrics_runtime_projections.rs.
mod metrics_runtime_projections {
    use cassie::app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent};
    use cassie::catalog::ProjectionVerificationState;

    use super::support_metrics as support;
    use support::{data_dir, use_local_storage};

    fn canonical_projection(cassie: &Cassie, projection: &str) -> String {
        cassie
            .catalog
            .get_schema(projection)
            .map_or_else(|| projection.to_string(), |schema| schema.collection)
    }

    fn projection_metric_delta(
        after: &serde_json::Value,
        before: &serde_json::Value,
        key: &str,
    ) -> u64 {
        after["projections"][key]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(before["projections"][key].as_u64().unwrap_or_default())
    }

    #[test]
    fn should_record_projection_replay_write_amplification() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_replay_write_amplification");
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
                    "CREATE TABLE projection_replay_metrics_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            let projection = canonical_projection(&cassie, "projection_replay_metrics_docs");

            let before = cassie.metrics();
            let events = vec![
                ProjectionReplayEvent {
                    event_id: "replay-write-amplification-1".to_string(),
                    checkpoint: "checkpoint-1".to_string(),
                    position: Some(1),
                    document_id: "doc-1".to_string(),
                    payload: Some(serde_json::json!({"title": "alpha"})),
                },
                ProjectionReplayEvent {
                    event_id: "replay-write-amplification-2".to_string(),
                    checkpoint: "checkpoint-2".to_string(),
                    position: Some(2),
                    document_id: "doc-2".to_string(),
                    payload: Some(serde_json::json!({"title": "bravo"})),
                },
            ];
            let batch = ProjectionReplayBatch {
                projection,
                source_identity: "replay-metrics-stream".to_string(),
                batch_id: "replay-metrics-batch".to_string(),
                lag: 0,
                events,
            };

            // Act
            let report = cassie.replay_projection_batch(batch).unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(report.applied_event_count, 2);
            assert_eq!(
                projection_metric_delta(&after, &before, "write_row_puts"),
                2
            );
            assert_eq!(
                projection_metric_delta(&after, &before, "write_metadata_puts"),
                2
            );
            assert_eq!(
                projection_metric_delta(&after, &before, "write_batch_flushes"),
                1
            );
            assert_eq!(
                projection_metric_delta(&after, &before, "replay_events_applied"),
                2
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_record_duplicate_replay_checks_without_row_puts() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_replay_duplicate_checks");
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
                    "CREATE TABLE projection_replay_duplicate_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            let projection = canonical_projection(&cassie, "projection_replay_duplicate_docs");

            let first = ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "replay-dup-stream".to_string(),
                batch_id: "replay-dup-first".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "replay-dup-event".to_string(),
                    checkpoint: "checkpoint-dup-1".to_string(),
                    position: Some(1),
                    document_id: "dup-doc".to_string(),
                    payload: Some(serde_json::json!({"title": "first"})),
                }],
            };
            cassie.replay_projection_batch(first).unwrap();

            let before = cassie.metrics();
            let second = ProjectionReplayBatch {
                projection,
                source_identity: "replay-dup-stream".to_string(),
                batch_id: "replay-dup-second".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "replay-dup-event".to_string(),
                    checkpoint: "checkpoint-dup-2".to_string(),
                    position: Some(2),
                    document_id: "dup-doc".to_string(),
                    payload: Some(serde_json::json!({"title": "replacement"})),
                }],
            };

            // Act
            let report = cassie.replay_projection_batch(second).unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(report.applied_event_count, 0);
            assert_eq!(report.skipped_duplicate_count, 1);
            assert_eq!(
                projection_metric_delta(&after, &before, "write_row_puts"),
                0
            );
            assert_eq!(
                projection_metric_delta(&after, &before, "write_row_deletes"),
                0
            );
            assert_eq!(
                projection_metric_delta(&after, &before, "write_duplicate_checks"),
                1
            );
            assert_eq!(
                projection_metric_delta(&after, &before, "replay_duplicates_skipped"),
                1
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_record_projection_rebuild_write_categories() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_rebuild_write_categories");
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
                "CREATE TABLE projection_rebuild_source_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_source_docs (title, score) VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .unwrap();

        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_rebuild_metric_projection AS SELECT title, score FROM projection_rebuild_source_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_source_docs (title, score) VALUES ('charlie', 3)",
                vec![],
            )
            .unwrap();
        let after_create = cassie.metrics();

        // Act
        cassie
            .execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_rebuild_metric_projection",
                vec![],
            )
            .unwrap();
        let after_refresh = cassie.metrics();

        // Assert
        assert_eq!(
            projection_metric_delta(&after_refresh, &after_create, "write_rebuild_target_puts"),
            3
        );
        assert_eq!(
            projection_metric_delta(&after_refresh, &after_create, "write_batch_flushes"),
            2
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_leave_projection_rebuild_hashes_current_after_refresh() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_rebuild_current_hashes");
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
                "CREATE TABLE projection_rebuild_hash_source (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_hash_source (title, score) VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_rebuild_hash_projection AS SELECT title, score FROM projection_rebuild_hash_source ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_hash_source (title, score) VALUES ('charlie', 3)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_rebuild_hash_projection",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .catalog
            .get_materialized_projection("projection_rebuild_hash_projection")
            .unwrap();
        let verification = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION projection_rebuild_hash_projection MODE full",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            metadata.hashes.root.state,
            ProjectionVerificationState::Current
        );
        assert_eq!(metadata.hashes.root.row_count, 3);
        assert_eq!(metadata.verification.state, ProjectionVerificationState::Verified);
        assert_eq!(
            verification.rows[0][0],
            cassie::types::Value::String("verified".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_record_projection_activation_metadata_write() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_activation_metadata_write");
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
                "CREATE TABLE projection_activation_source_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_activation_metric_projection AS SELECT title, score FROM projection_activation_source_docs",
                vec![],
            )
            .unwrap();

        let before = cassie.metrics();

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED PROJECTION projection_activation_metric_projection BUILD VERSION",
                vec![],
            )
            .unwrap();
        let version_id = cassie
            .catalog
            .get_materialized_projection("projection_activation_metric_projection")
            .and_then(|metadata| {
                metadata
                    .versions
                    .last()
                    .map(|version| version.version_id.clone())
            })
            .unwrap_or_else(|| "v1".to_string());

        cassie
            .execute_sql(
                &session,
                &format!(
                    "ALTER MATERIALIZED PROJECTION projection_activation_metric_projection ACTIVATE VERSION {version_id}"
                ),
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            projection_metric_delta(&after, &before, "write_activation_metadata_writes"),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/metrics_search.rs.
mod metrics_search {
    #![allow(unused_imports, dead_code)]

    use super::support_pgwire as pgwire_support;

    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::runtime::RuntimeFeedbackKey;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{data_dir, describe_statement_frame, startup_frame, use_local_storage};

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

    fn register_collection_with_schema(cassie: &Cassie, collection: &str, schema: &Schema) {
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
    }

    fn insert_documents(cassie: &Cassie, collection: &str, docs: Vec<(&str, serde_json::Value)>) {
        for (id, payload) in docs {
            cassie
                .midge
                .put_document(collection, Some(id.to_string()), payload)
                .unwrap();
        }
    }

    fn adaptive_candidate_config(min: usize, max: usize) -> cassie::config::CassieRuntimeConfig {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.adaptive_candidate_min = min;
        config.limits.adaptive_candidate_max = max;
        config
    }

    fn assert_prefilter_metrics(before: &serde_json::Value, after: &serde_json::Value) {
        let before_input = before["vector"]["prefilter_input_candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_filtered = before["vector"]["prefilter_filtered_candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_fallback = before["vector"]["prefilter_fallback_count_total"]
            .as_u64()
            .unwrap_or_default();
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
    }

    fn assert_fulltext_scoring_cache_metrics(
        before: &serde_json::Value,
        after_first: &serde_json::Value,
        after_second: &serde_json::Value,
        after_third: &serde_json::Value,
    ) {
        let before_hits = before["query_cache"]["fulltext_stats_hits"]
            .as_u64()
            .unwrap_or_default();
        let before_misses = before["query_cache"]["fulltext_stats_misses"]
            .as_u64()
            .unwrap_or_default();
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
        pgwire_support::read_wire_frame(reader).await
    }

    #[test]
    fn should_record_vector_counts_for_ordered_search_expression() {
        // Arrange
        use_local_storage();
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
        use_local_storage();
        let path = data_dir("vector_prefilter_counts");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_vector_prefilter_counts";
        register_collection_with_schema(&cassie, collection, &Schema {
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
        });
        insert_documents(
            &cassie,
            collection,
            vec![
                (
                    "doc-1",
                    serde_json::json!({
                        "status": "approved",
                        "embedding": [1.0, 0.0],
                    }),
                ),
                (
                    "doc-2",
                    serde_json::json!({
                        "status": "approved",
                        "embedding": [2.0, 0.0],
                    }),
                ),
                (
                    "doc-3",
                    serde_json::json!({
                        "status": "pending",
                        "embedding": [3.0, 0.0],
                    }),
                ),
            ],
        );

        let before = cassie.metrics();
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
        assert_prefilter_metrics(&before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_record_search_operator_statistics() {
        // Arrange
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
        let path = data_dir("fulltext_scoring_metadata_cache");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_fulltext_scoring_metadata_cache";
        register_collection_with_schema(&cassie, collection, &Schema {
            fields: vec![FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        });
        insert_documents(
            &cassie,
            collection,
            vec![
                ("doc-1", serde_json::json!({"body": "alpha bravo"})),
                ("doc-2", serde_json::json!({"body": "alpha charlie"})),
            ],
        );

        let before = cassie.metrics();
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
        assert_fulltext_scoring_cache_metrics(
            &before,
            &after_first,
            &after_second,
            &after_third,
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}
