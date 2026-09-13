// Consolidated integration suite: analytics.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/failpoints.rs"]
mod support_failpoints;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/aggregate_acceleration.rs.
mod aggregate_acceleration {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_accelerate_numeric_aggregates_from_column_summaries() {
        // Arrange
        use_local_storage();
        let path = data_dir("aggregate_accel_numeric");
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
                "CREATE TABLE aggregate_accel_numeric (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_accel_numeric (amount) VALUES (7)",
            "INSERT INTO aggregate_accel_numeric (amount) VALUES (NULL)",
            "INSERT INTO aggregate_accel_numeric (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_aggregate_accel_numeric ON aggregate_accel_numeric USING column (amount) WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS rows, COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_accel_numeric",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT COUNT(*) AS rows, SUM(amount) AS total FROM aggregate_accel_numeric",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Int64(3),
                Value::Int64(2),
                Value::Int64(10),
                Value::Float64(5.0),
                Value::Int64(3),
                Value::Int64(7),
            ]]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(
            plan.contains("aggregate_acceleration=true"),
            "plan was: {plan}"
        );
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 1);
        assert_eq!(
            metrics["aggregate_acceleration"]["accelerated_segments"],
            2
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_maintain_aggregate_summaries_after_update_delete() {
        // Arrange
        use_local_storage();
        let path = data_dir("aggregate_accel_maintenance");
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
                "CREATE TABLE aggregate_accel_maintenance (amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_accel_maintenance (amount) VALUES (2)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_accel_maintenance (amount) VALUES (4)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_aggregate_accel_maintenance ON aggregate_accel_maintenance USING column (amount) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE aggregate_accel_maintenance SET amount = 10 WHERE amount = 2",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM aggregate_accel_maintenance WHERE amount = 4",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS rows, SUM(amount) AS total, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_accel_maintenance",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::Int64(10),
                Value::Int64(10),
            ]]
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_to_row_blobs_for_grouped_aggregates() {
        // Arrange
        use_local_storage();
        let path = data_dir("aggregate_accel_group_fallback");
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
                "CREATE TABLE aggregate_accel_group_fallback (category TEXT, amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_accel_group_fallback (category, amount) VALUES ('a', 7)",
            "INSERT INTO aggregate_accel_group_fallback (category, amount) VALUES ('a', 3)",
            "INSERT INTO aggregate_accel_group_fallback (category, amount) VALUES ('b', 5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_aggregate_accel_group_fallback ON aggregate_accel_group_fallback USING column (category, amount) WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, SUM(amount) AS total FROM aggregate_accel_group_fallback GROUP BY category ORDER BY category",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT category, SUM(amount) AS total FROM aggregate_accel_group_fallback GROUP BY category ORDER BY category",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("a".to_string()), Value::Int64(10)],
                vec![Value::String("b".to_string()), Value::Int64(5)],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("aggregate_acceleration=false"));
    });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/analytical_projection_recovery.rs.
mod analytical_projection_recovery {
    use cassie::app::Cassie;
    use cassie::app::CassieSession;
    use cassie::executor::set_materialized_projection_maintenance_failure_point;
    use cassie::types::Value;
    use std::path::Path;

    static MATERIALIZED_PROJECTION_FAILPOINT_GUARD: std::sync::Mutex<()> =
        std::sync::Mutex::new(());

    use super::support_sql as support;
    use support::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn setup_source_and_projection(path: &Path) -> (Cassie, CassieSession) {
        let cassie = Cassie::new_with_data_dir(path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE analytical_debt_source (tenant TEXT, amount INT)",
                vec![],
            )
            .expect("create source");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO analytical_debt_source (tenant, amount) VALUES ('acme', 10)",
                vec![],
            )
            .expect("seed source");
        cassie
        .execute_sql(
            &session,
            "CREATE MATERIALIZED PROJECTION analytical_debt WITH (analytical = true) AS SELECT tenant, amount FROM analytical_debt_source",
            vec![],
        )
        .expect("create projection");
        (cassie, session)
    }

    #[test]
    fn should_replay_materialized_projection_debt_after_post_commit_stale_failure() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = MATERIALIZED_PROJECTION_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("analytical_projection_maintenance_debt");

        runtime().block_on(async {
        let (cassie, session) = setup_source_and_projection(path.as_ref());
        // Act
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        let insert = cassie
            .execute_sql(
                &session,
                "INSERT INTO analytical_debt_source (tenant, amount) VALUES ('acme', 20)",
                vec![],
            )
            .expect("stage write");
        set_materialized_projection_maintenance_failure_point(true);
        let commit = cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("base commit remains successful");
        let debt = cassie
            .execute_sql(
                &session,
                "SELECT artifact, target_generation, retry_count, last_error, fallback_reason FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.analytical_debt_source'",
                vec![],
            )
            .expect("read maintenance debt");
        let before_restart = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM analytical_debt_source ORDER BY amount",
                vec![],
            )
            .expect("read source fallback");
        let before_restart_metrics = cassie.metrics();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("restart cassie");
        restarted.startup().expect("restart startup");
        let restarted_session = restarted.create_session("tester", None);
        let restarted_debt = restarted
            .execute_sql(
                &restarted_session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.analytical_debt_source'",
                vec![],
            )
            .expect("read replayed debt");
        let projection = restarted
            .execute_sql(
                &restarted_session,
                "SELECT state FROM pg_catalog.pg_materialized_projections WHERE projection_name = 'postgres.public.analytical_debt'",
                vec![],
            )
            .expect("read projection state");
        let after_restart = restarted
            .execute_sql(
                &restarted_session,
                "SELECT tenant, amount FROM analytical_debt_source ORDER BY amount",
                vec![],
            )
            .expect("read source after restart");
        let after_restart_metrics = restarted.metrics();

        // Assert
        assert_eq!(insert.command, "INSERT 0 1");
        assert_eq!(commit.command, "COMMIT");
        assert_eq!(before_restart.rows.len(), 2);
        assert_eq!(
            before_restart_metrics["projections"]["last_fallback_reason"].as_str(),
            Some("maintenance_pending")
        );
        assert_eq!(debt.rows.len(), 1);
        assert_eq!(
            debt.rows[0],
            vec![
                Value::String("materialized_projection".to_string()),
                Value::Int64(2),
                Value::Int64(1),
                Value::String(
                    "materialized_projection maintenance failed (details redacted)".to_string(),
                ),
                Value::String("maintenance_pending".to_string()),
            ]
        );
        assert!(restarted_debt.rows.is_empty());
        assert_eq!(
            projection.rows,
            vec![vec![Value::String("stale".to_string())]]
        );
        assert_eq!(after_restart.rows.len(), 2);
        assert_eq!(
            after_restart_metrics["projections"]["last_fallback_reason"].as_str(),
            Some("stale-or-unverified")
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_analytical_projection_with_mismatched_source_generation() {
        // Arrange
        use_local_storage();
        let path = data_dir("analytical_projection_generation_fence");

        runtime().block_on(async {
            let (cassie, session) = setup_source_and_projection(path.as_ref());
            let mut projection = cassie
                .catalog
                .get_materialized_projection("analytical_debt")
                .expect("projection metadata");
            projection
                .source_generations
                .insert("postgres.public.analytical_debt_source".to_string(), 0);
            cassie.midge.put_projection_metadata(&projection).unwrap();
            cassie.catalog.register_projection_metadata(projection);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT tenant, amount FROM analytical_debt_source ORDER BY amount",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0][1], Value::Int64(10));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/analytical_projections.rs.
mod analytical_projections {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::catalog::local_name;
    use cassie::sql::ast::QueryStatement;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_parse_analytical_materialized_projection_options() {
        // Arrange
        let sql = "CREATE MATERIALIZED PROJECTION sales_daily WITH (analytical = true, column_storage = true, partition_by = tenant, sort_by = event_at, refresh = manual) AS SELECT tenant, event_at FROM sales";

        // Act
        let parsed = cassie::sql::parse_statement(sql).unwrap();

        // Assert
        let QueryStatement::CreateMaterializedProjection(statement) = parsed.statement else {
            panic!("expected CREATE MATERIALIZED PROJECTION");
        };
        assert_eq!(statement.name, "sales_daily");
        assert_eq!(
            statement.options.get("analytical"),
            Some(&"true".to_string())
        );
        assert_eq!(
            statement.options.get("column_storage"),
            Some(&"true".to_string())
        );
        assert_eq!(
            statement.options.get("refresh"),
            Some(&"manual".to_string())
        );
    }

    #[test]
    fn should_persist_analytical_projection_options() {
        // Arrange
        use_local_storage();
        let path = data_dir("analytical_projection_options");
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
                "CREATE TABLE analytical_source (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION analytical_projection WITH (analytical = true, column_storage = true, partition_by = tenant, sort_by = event_at, refresh = manual) AS SELECT tenant, event_at, amount FROM analytical_source",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .catalog
            .get_materialized_projection("analytical_projection")
            .expect("materialized projection metadata");

        // Assert
        let materialized = metadata.materialized.expect("materialized metadata");
        assert_eq!(
            materialized.options.get("analytical"),
            Some(&"true".to_string())
        );
        assert_eq!(
            materialized.options.get("column_storage"),
            Some(&"true".to_string())
        );
        assert_eq!(
            materialized.options.get("partition_by"),
            Some(&"tenant".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_route_covered_query_to_fresh_analytical_projection() {
        // Arrange
        use_local_storage();
        let path = data_dir("analytical_projection_route");
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
                "CREATE TABLE analytical_route_source (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO analytical_route_source (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 20), ('other', '2026-01-01T01:00:00Z', 5), ('acme', '2026-01-01T02:00:00Z', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION analytical_route WITH (analytical = true, column_storage = true, partition_by = tenant, sort_by = event_at, refresh = manual) AS SELECT tenant, event_at, amount FROM analytical_route_source",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM analytical_route_source WHERE tenant = 'acme' ORDER BY amount",
                vec![],
            )
            .unwrap();
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT tenant, amount FROM analytical_route_source WHERE tenant = 'acme' ORDER BY amount",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();
        let projection = cassie
            .catalog
            .get_materialized_projection("analytical_route")
            .expect("materialized projection metadata");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("acme".to_string()), Value::Int64(10)],
                vec![Value::String("acme".to_string()), Value::Int64(20)],
            ]
        );
        assert!(
            after["projections"]["mixed_execution_optimized"]
                .as_u64()
                .unwrap()
                > before["projections"]["mixed_execution_optimized"]
                    .as_u64()
                    .unwrap()
        );
        let Value::String(plan) = &explained.rows[0][0] else {
            panic!("expected explain string");
        };
        assert!(plan.contains(&format!(
            "analytical_projection={}",
            projection.collection
        )));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fallback_to_source_when_analytical_projection_is_stale() {
        // Arrange
        use_local_storage();
        let path = data_dir("analytical_projection_stale_fallback");
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
                "CREATE TABLE analytical_stale_source (tenant TEXT, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO analytical_stale_source (tenant, amount) VALUES ('acme', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION analytical_stale WITH (analytical = true) AS SELECT tenant, amount FROM analytical_stale_source",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO analytical_stale_source (tenant, amount) VALUES ('acme', 20)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM analytical_stale_source WHERE tenant = 'acme' ORDER BY amount",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("acme".to_string()), Value::Int64(10)],
                vec![Value::String("acme".to_string()), Value::Int64(20)],
            ]
        );
        assert!(
            after["projections"]["mixed_execution_fallbacks"]
                .as_u64()
                .unwrap()
                > before["projections"]["mixed_execution_fallbacks"]
                    .as_u64()
                    .unwrap()
        );
        assert_eq!(
            after["projections"]["last_fallback_reason"].as_str(),
            Some("stale-or-unverified")
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_dml_against_analytical_projection_output() {
        // Arrange
        use_local_storage();
        let path = data_dir("analytical_projection_output_dml");
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
                "CREATE TABLE analytical_output_source (tenant TEXT, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION analytical_output WITH (analytical = true) AS SELECT tenant, amount FROM analytical_output_source",
                vec![],
            )
            .unwrap();
        let output = cassie
            .catalog
            .get_materialized_projection("analytical_output")
            .and_then(|projection| projection.active_output_collection().map(str::to_string))
            .expect("active output collection");
        let output_sql_name = local_name(&output);

        // Act
        let error = cassie
            .execute_sql(
                &session,
                &format!(
                    "INSERT INTO {output_sql_name} (tenant, amount) VALUES ('acme', 10)"
                ),
                vec![],
            )
            .unwrap_err();

        // Assert
        assert!(error.to_string().contains("read-only"));

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/cardinality_generation.rs.
mod cardinality_generation {
    use cassie::app::Cassie;
    use cassie::types::{DataType, FieldSchema, Schema};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    #[test]
    fn should_reject_cardinality_stats_from_an_older_collection_generation() {
        // Arrange
        use_local_storage();
        let path = data_dir("cardinality_generation");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = "cardinality_generation_docs";
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
            .expect("create collection");
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
                serde_json::json!({"title": "before"}),
            )
            .expect("seed document");
        cassie
            .midge
            .rebuild_cardinality_stats_for_collection(collection)
            .expect("build current stats");
        assert!(cassie
            .midge
            .get_cardinality_stats(collection)
            .expect("read current stats")
            .is_some());

        // Act
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"title": "after"}),
            )
            .expect("advance collection generation");

        // Assert
        assert!(cassie
            .midge
            .get_cardinality_stats(collection)
            .expect("read stale stats")
            .is_none());

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/cardinality_incremental.rs.
mod cardinality_incremental {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::catalog::CollectionCardinalityStats;

    use super::support_sql as support;

    use support::{canonical_test_collection, data_dir, use_local_storage};

    fn metric_delta(after: &serde_json::Value, before: &serde_json::Value, key: &str) -> u64 {
        after["cardinality"][key]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(before["cardinality"][key].as_u64().unwrap_or_default())
    }

    #[test]
    fn should_increment_cardinality_for_unindexed_document_write_without_rebuild() {
        // Arrange
        use_local_storage();
        let path = data_dir("incremental_unindexed_cardinality");
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
                    "CREATE TABLE incremental_docs (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            let collection = canonical_test_collection(&cassie, "incremental_docs");
            let before = cassie.metrics();

            // Act
            let id = cassie
                .ingest_document(
                    &collection,
                    serde_json::json!({"title": "alpha", "body": "bravo"}),
                )
                .unwrap();
            let after_insert = cassie.metrics();
            let insert_stats = cassie
                .catalog
                .get_cardinality_stats(&collection)
                .expect("insert cardinality stats");
            cassie::rest::documents::delete(&cassie, &collection, &id).unwrap();
            let after_delete = cassie.metrics();

            // Assert
            assert_eq!(insert_stats.row_count, 1);
            assert!(insert_stats.hydrated);
            let delete_stats = cassie
                .catalog
                .get_cardinality_stats(&collection)
                .expect("incremental cardinality stats");
            assert_eq!(delete_stats.row_count, 0);
            assert!(delete_stats.hydrated);
            assert_eq!(metric_delta(&after_insert, &before, "rebuilds"), 0);
            assert_eq!(metric_delta(&after_insert, &before, "writes"), 1);
            assert_eq!(metric_delta(&after_delete, &before, "rebuilds"), 0);
            assert_eq!(metric_delta(&after_delete, &before, "writes"), 2);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_rebuild_cardinality_when_index_membership_changes() {
        // Arrange
        use_local_storage();
        let path = data_dir("indexed_cardinality_rebuild");
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
                    "CREATE TABLE indexed_docs (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            let collection = canonical_test_collection(&cassie, "indexed_docs");
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_indexed_title ON indexed_docs USING btree (title)",
                    vec![],
                )
                .unwrap();
            let before = cassie.metrics();

            // Act
            cassie
                .ingest_document(
                    &collection,
                    serde_json::json!({"title": "alpha", "body": "bravo"}),
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            let stats = cassie
                .catalog
                .get_cardinality_stats(&collection)
                .expect("indexed cardinality stats");
            let index = cassie
                .catalog
                .get_index(&collection, "idx_indexed_title")
                .expect("index metadata");
            assert_eq!(stats.row_count, 1);
            assert_eq!(
                stats.index_cardinality(&CollectionCardinalityStats::scalar_index_key(&index.name)),
                Some(1)
            );
            assert_eq!(metric_delta(&after, &before, "rebuilds"), 1);
            assert_eq!(metric_delta(&after, &before, "writes"), 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/column_batch_controls.rs.
mod column_batch_controls {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

    use super::support_sql as support;

    const COLLECTION: &str = "controlled_column_metadata";
    const METADATA_REJECTION_BUDGET: usize = 1_024;

    struct Fixture {
        cassie: Cassie,
        path: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn fixture() -> Fixture {
        support::use_local_storage();
        let path = support::data_dir("column-metadata-controls");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = METADATA_REJECTION_BUDGET;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("controlled cassie");
        cassie.startup().expect("startup controlled cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {COLLECTION} (score INT, label TEXT)"),
                vec![],
            )
            .expect("create controlled table");
        let rows = (0..64)
            .map(|index| {
                (
                    Some(format!("row-{index:04}")),
                    serde_json::json!({
                        "score": index,
                        "label": format!("label-{index:04}-{}", "x".repeat(512)),
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(COLLECTION, rows)
            .expect("seed controlled rows");
        cassie
            .execute_sql(
                &session,
                &format!(
                    "CREATE INDEX controlled_column_metadata_idx ON {COLLECTION} USING column \
                 (score, label) WITH (segment_size = 1)"
                ),
                vec![],
            )
            .expect("create metadata-heavy column index");
        Fixture { cassie, path }
    }

    fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
        metrics[family][name].as_u64().unwrap_or_default()
    }

    fn execute_with_first_segment_cancellation(
        fixture: &Fixture,
        sql: &str,
    ) -> (Result<cassie::executor::QueryResult, CassieError>, u64) {
        let session = fixture.cassie.create_session("reader", None);
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(1));
        let result = fixture.cassie.execute_sql(&session, sql, vec![]);
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(None);
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);
        (result, reads)
    }

    fn assert_resource_rejection_before_segment_reads(
        fixture: &Fixture,
        result: Result<cassie::executor::QueryResult, CassieError>,
        reads: u64,
        before: &serde_json::Value,
        metric_family: &str,
    ) {
        let error = result.expect_err("metadata-heavy query should reject before segment reads");
        assert!(matches!(error, CassieError::ResourceLimit(_)), "{error:?}");
        assert_eq!(reads, 0, "segment scan started before metadata reservation");
        let after = fixture.cassie.metrics();
        assert_eq!(
            metric(&after, metric_family, "scans"),
            metric(before, metric_family, "scans"),
            "failed metadata path published success"
        );
        assert_eq!(metric(&after, "runtime", "running_queries"), 0);
        assert_eq!(metric(&after, "query", "current_accounted_memory_bytes"), 0);
    }

    #[test]
    fn should_reserve_projection_metadata_before_loading_column_segments() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        // Arrange
        let fixture = fixture();
        let before = fixture.cassie.metrics();

        // Act
        let (result, reads) = execute_with_first_segment_cancellation(
            &fixture,
            &format!("SELECT id, score, label FROM {COLLECTION} WHERE score >= 0 LIMIT 5"),
        );

        // Assert
        assert_resource_rejection_before_segment_reads(
            &fixture,
            result,
            reads,
            &before,
            "column_batches",
        );
    }

    #[test]
    fn should_reserve_aggregate_metadata_before_validating_column_segments() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        // Arrange
        let fixture = fixture();
        let before = fixture.cassie.metrics();

        // Act
        let (result, reads) = execute_with_first_segment_cancellation(
            &fixture,
            &format!("SELECT COUNT(*), SUM(score), AVG(score) FROM {COLLECTION}"),
        );

        // Assert
        assert_resource_rejection_before_segment_reads(
            &fixture,
            result,
            reads,
            &before,
            "aggregate_acceleration",
        );
    }
}

// Formerly tests/column_batch_encoded_aggregates.rs.
mod column_batch_encoded_aggregates {
    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    #[test]
    fn should_accelerate_filtered_numeric_aggregates_over_encoded_selection() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_filtered_aggregate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE filtered_aggregate (status TEXT, amount INT)",
                    vec![],
                )
                .expect("create table");
            for sql in [
                "INSERT INTO filtered_aggregate (status, amount) VALUES ('active', 7)",
                "INSERT INTO filtered_aggregate (status, amount) VALUES ('active', NULL)",
                "INSERT INTO filtered_aggregate (status, amount) VALUES ('active', 3)",
                "INSERT INTO filtered_aggregate (status, amount) VALUES ('inactive', 100)",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("insert row");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX filtered_aggregate_idx ON filtered_aggregate \
                 USING column (status, amount) WITH (segment_size = 4)",
                    vec![],
                )
                .expect("create column index");
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT COUNT(*) AS rows, COUNT(amount) AS present, \
                 SUM(amount) AS total, AVG(amount) AS average, \
                 MIN(amount) AS smallest, MAX(amount) AS largest \
                 FROM filtered_aggregate WHERE status = 'active'",
                    vec![],
                )
                .expect("execute filtered aggregate");
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT SUM(amount) AS total \
                 FROM filtered_aggregate WHERE status = 'active'",
                    vec![],
                )
                .expect("explain filtered aggregate");
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![vec![
                    Value::Int64(3),
                    Value::Int64(2),
                    Value::Int64(10),
                    Value::Float64(5.0),
                    Value::Int64(3),
                    Value::Int64(7),
                ]]
            );
            assert_eq!(
                after["aggregate_acceleration"]["scans"]
                    .as_u64()
                    .unwrap_or_default()
                    - before["aggregate_acceleration"]["scans"]
                        .as_u64()
                        .unwrap_or_default(),
                1
            );
            assert_eq!(
                after["column_batches"]["selected_rows"]
                    .as_u64()
                    .unwrap_or_default()
                    - before["column_batches"]["selected_rows"]
                        .as_u64()
                        .unwrap_or_default(),
                3
            );
            let Value::String(plan) = &explain.rows[0][0] else {
                panic!("expected textual plan");
            };
            assert!(plan.contains("aggregate_acceleration=true"));
            assert!(plan.contains("encoded_execution=true"));
            assert!(plan.contains("predicate_fields=status"));
            assert!(plan.contains("projection_fields=amount"));
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_empty_filtered_aggregate_semantics() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_empty_filtered_aggregate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE empty_filtered_aggregate (status TEXT, amount INT)",
                    vec![],
                )
                .expect("create table");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO empty_filtered_aggregate (status, amount) VALUES ('active', 7)",
                    vec![],
                )
                .expect("insert row");
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX empty_filtered_aggregate_idx ON empty_filtered_aggregate \
                 USING column (status, amount) WITH (segment_size = 4)",
                    vec![],
                )
                .expect("create column index");

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT COUNT(*) AS rows, COUNT(amount) AS present, \
                 SUM(amount) AS total, AVG(amount) AS average, \
                 MIN(amount) AS smallest, MAX(amount) AS largest \
                 FROM empty_filtered_aggregate WHERE status = 'missing'",
                    vec![],
                )
                .expect("execute empty filtered aggregate");

            // Assert
            assert_eq!(
                result.rows,
                vec![vec![
                    Value::Int64(0),
                    Value::Int64(0),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                ]]
            );
            assert_eq!(
                cassie.metrics()["aggregate_acceleration"]["scans"],
                serde_json::json!(1)
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/column_batch_encoded_scans.rs.
mod column_batch_encoded_scans {
    use cassie::app::{Cassie, CassieSession};
    use cassie::midge::adapter::{decode_column_chunk_for_test, StorageFamily};
    use cassie::types::Value;
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn metric(metrics: &serde_json::Value, name: &str) -> u64 {
        metrics["column_batches"][name].as_u64().unwrap_or_default()
    }

    struct EncodedScanFixture {
        cassie: Cassie,
        session: CassieSession,
    }

    fn corrupted_isolation_fixture(path: &str) -> EncodedScanFixture {
        let cassie = Cassie::new_with_data_dir(path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE encoded_scan_isolation (status TEXT, amount INT, ignored TEXT)",
                vec![],
            )
            .expect("create table");
        for amount in 0..8 {
            let status = if amount % 2 == 0 {
                "active"
            } else {
                "inactive"
            };
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO encoded_scan_isolation (status, amount, ignored) \
                     VALUES ('{status}', {amount}, 'poison-{amount}')"
                    ),
                    vec![],
                )
                .expect("insert row");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX encoded_scan_isolation_idx ON encoded_scan_isolation \
             USING column (status, amount, ignored) WITH (segment_size = 8)",
                vec![],
            )
            .expect("create column index");
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan raw column chunks");
        let (ignored_key, mut ignored_chunk) = entries
            .into_iter()
            .find(|(_, value)| {
                value.starts_with(b"CBC2")
                    && decode_column_chunk_for_test(value).is_ok_and(|values| {
                        values.iter().all(|value| {
                            value
                                .as_str()
                                .is_some_and(|value| value.starts_with("poison-"))
                        })
                    })
            })
            .expect("find ignored field chunk");
        let last = ignored_chunk
            .last_mut()
            .expect("encoded chunk should not be empty");
        *last ^= 0x80;
        let mut tx = cassie
            .midge
            .data_tx(TransactionMode::ReadWrite)
            .expect("open write transaction");
        tx.put(ignored_key, ignored_chunk, None)
            .expect("corrupt ignored field");
        tx.commit(WriteOptions::sync())
            .expect("commit field corruption");
        EncodedScanFixture { cassie, session }
    }

    #[test]
    fn should_late_materialize_selected_values_despite_unrequested_corruption() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_encoded_scan_isolation");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let fixture = corrupted_isolation_fixture(&path);
            let cassie = fixture.cassie;
            let session = fixture.session;
            let before = cassie.metrics();

            // Act
            let covered = cassie
                .execute_sql(
                    &session,
                    "SELECT amount FROM encoded_scan_isolation \
                 WHERE status = 'active' ORDER BY amount",
                    vec![],
                )
                .expect("scan covered fields");
            let after_covered = cassie.metrics();
            let required = cassie
                .execute_sql(
                    &session,
                    "SELECT ignored FROM encoded_scan_isolation \
                 WHERE status = 'active' ORDER BY amount",
                    vec![],
                )
                .expect("fall back for corrupt required field");
            let after_required = cassie.metrics();

            // Assert
            assert_eq!(
                covered.rows,
                vec![
                    vec![Value::Int64(0)],
                    vec![Value::Int64(2)],
                    vec![Value::Int64(4)],
                    vec![Value::Int64(6)],
                ]
            );
            assert_eq!(
                metric(&after_covered, "scans") - metric(&before, "scans"),
                1
            );
            assert_eq!(
                metric(&after_covered, "chunks_read") - metric(&before, "chunks_read"),
                3
            );
            assert_eq!(
                metric(&after_covered, "predicate_values") - metric(&before, "predicate_values"),
                8
            );
            assert_eq!(
                metric(&after_covered, "selected_rows") - metric(&before, "selected_rows"),
                4
            );
            assert_eq!(
                metric(&after_covered, "materialized_values")
                    - metric(&before, "materialized_values"),
                8
            );
            assert_eq!(
                metric(&after_covered, "decode_fallbacks") - metric(&before, "decode_fallbacks"),
                0
            );
            assert_eq!(
                required.rows,
                vec![
                    vec![Value::String("poison-0".to_string())],
                    vec![Value::String("poison-2".to_string())],
                    vec![Value::String("poison-4".to_string())],
                    vec![Value::String("poison-6".to_string())],
                ]
            );
            assert_eq!(
                metric(&after_required, "decode_fallbacks")
                    - metric(&after_covered, "decode_fallbacks"),
                1
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_apply_encoded_conjunctions_with_null_predicates_exactly() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_encoded_conjunctions");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE encoded_conjunctions (label TEXT, score INT, note TEXT)",
                    vec![],
                )
                .expect("create table");
            for score in 0..6 {
                let note = if score == 3 {
                    "NULL".to_string()
                } else {
                    format!("'note-{score}'")
                };
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "INSERT INTO encoded_conjunctions (label, score, note) \
                         VALUES ('row-{score}', {score}, {note})"
                        ),
                        vec![],
                    )
                    .expect("insert row");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX encoded_conjunctions_idx ON encoded_conjunctions \
                 USING column (label, score, note) WITH (segment_size = 6)",
                    vec![],
                )
                .expect("create column index");
            let before = cassie.metrics();

            // Act
            let non_null = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM encoded_conjunctions \
                 WHERE score >= 2 AND score < 5 AND note IS NOT NULL ORDER BY label",
                    vec![],
                )
                .expect("execute non-null conjunction");
            let null = cassie
                .execute_sql(
                    &session,
                    "SELECT label FROM encoded_conjunctions \
                 WHERE score >= 2 AND score < 5 AND note IS NULL ORDER BY label",
                    vec![],
                )
                .expect("execute null conjunction");
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                non_null.rows,
                vec![
                    vec![Value::String("row-2".to_string())],
                    vec![Value::String("row-4".to_string())],
                ]
            );
            assert_eq!(null.rows, vec![vec![Value::String("row-3".to_string())]]);
            assert_eq!(metric(&after, "scans") - metric(&before, "scans"), 2);
            assert_eq!(
                metric(&after, "fallback_scans") - metric(&before, "fallback_scans"),
                0
            );
            assert_eq!(
                metric(&after, "selected_rows") - metric(&before, "selected_rows"),
                3
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/column_batch_format_v2.rs.
mod column_batch_format_v2 {
    use cassie::catalog::ColumnBatchMetadata;
    use cassie::midge::adapter::{
        column_chunk_codec_for_test, decode_column_batch_manifest_for_test,
        decode_column_chunk_for_test, decode_row_id_chunk_for_test,
        decode_selected_column_chunk_for_test, encode_column_batch_manifest_for_test,
        encode_column_chunk_for_test, encode_plain_column_chunk_for_test,
        encode_row_id_chunk_for_test,
    };

    fn encode(logical_type: &str, values: &[serde_json::Value]) -> Vec<u8> {
        encode_column_chunk_for_test(logical_type, values).expect("encode column chunk")
    }

    #[test]
    fn should_emit_platform_independent_little_endian_plain_integer_bytes() {
        // Arrange
        let values = [serde_json::json!(1), serde_json::json!(-2)];

        // Act
        let encoded = encode("bigint", &values);

        // Assert
        assert_eq!(
            encoded,
            vec![
                b'C', b'B', b'C', b'2', 2, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 16,
                0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 3, 1, 0, 0, 0, 0, 0, 0, 0, 254, 255, 255, 255,
                255, 255, 255, 255,
            ]
        );
    }

    #[test]
    fn should_roundtrip_sparse_validity_with_signed_extremes() {
        // Arrange
        let values = [
            serde_json::json!(i64::MIN),
            serde_json::Value::Null,
            serde_json::json!(0),
            serde_json::Value::Null,
            serde_json::json!(i64::MAX),
        ];

        // Act
        let encoded = encode("bigint", &values);
        let decoded = decode_column_chunk_for_test(&encoded).expect("decode integer chunk");

        // Assert
        assert_eq!(decoded, values);
    }

    #[test]
    fn should_select_each_typed_codec_deterministically() {
        // Arrange
        let constant = vec![serde_json::json!(7); 256];
        let rle = (0..8)
            .flat_map(|value| std::iter::repeat_n(serde_json::json!(format!("run-{value}")), 64))
            .collect::<Vec<_>>();
        let dictionary = (0..512)
            .map(|position| serde_json::json!(format!("status-{}", position % 4)))
            .collect::<Vec<_>>();
        let frame_of_reference = (0..512)
            .map(|value| serde_json::json!(10_000_i64 + i64::from(value)))
            .collect::<Vec<_>>();

        // Act
        let constant_codec =
            column_chunk_codec_for_test(&encode("bigint", &constant)).expect("constant codec");
        let rle_codec = column_chunk_codec_for_test(&encode("text", &rle)).expect("rle codec");
        let dictionary_codec =
            column_chunk_codec_for_test(&encode("text", &dictionary)).expect("dictionary codec");
        let for_codec =
            column_chunk_codec_for_test(&encode("bigint", &frame_of_reference)).expect("for codec");

        // Assert
        assert_eq!(constant_codec, "constant");
        assert_eq!(rle_codec, "rle");
        assert_eq!(dictionary_codec, "dictionary");
        assert_eq!(for_codec, "frame_of_reference");
    }

    #[test]
    fn should_roundtrip_zero_width_frame_of_reference_blocks() {
        // Arrange
        let values = (0..256).map(|_| serde_json::json!(42)).collect::<Vec<_>>();

        // Act
        let encoded = cassie::midge::adapter::encode_for_column_chunk_for_test(&values)
            .expect("encode forced FOR chunk");
        let decoded = decode_column_chunk_for_test(&encoded).expect("decode zero-width FOR");

        // Assert
        assert_eq!(decoded, values);
    }

    #[test]
    fn should_limit_float_codec_selection_to_supported_encodings() {
        // Arrange
        let values = (0..512)
            .map(|value| serde_json::json!(f64::from(value) / 10.0))
            .collect::<Vec<_>>();

        // Act
        let encoded = encode("float", &values);
        let codec = column_chunk_codec_for_test(&encoded).expect("float codec");
        let decoded = decode_column_chunk_for_test(&encoded).expect("decode floats");

        // Assert
        assert!(matches!(codec, "plain" | "constant" | "rle" | "alp"));
        assert_eq!(decoded, values);
    }

    #[test]
    fn should_roundtrip_repeated_utf8_with_fsst() {
        // Arrange
        let values = (0..512)
            .map(|position| {
                serde_json::json!(format!(
                    "tenant-{}/event-{}-payload-{}",
                    position % 32,
                    position,
                    position % 8
                ))
            })
            .collect::<Vec<_>>();

        // Act
        let encoded = encode("text", &values);
        let codec = column_chunk_codec_for_test(&encoded).expect("text codec");
        let decoded = decode_column_chunk_for_test(&encoded).expect("decode FSST text");

        // Assert
        assert_eq!(codec, "fsst");
        assert_eq!(decoded, values);
    }

    #[test]
    fn should_roundtrip_alp_finite_values_bit_exactly() {
        // Arrange
        let values = (0..512)
            .map(|position| serde_json::json!((f64::from(position) - 256.0) / 100.0))
            .collect::<Vec<_>>();

        // Act
        let encoded = encode("float", &values);
        let codec = column_chunk_codec_for_test(&encoded).expect("float codec");
        let decoded = decode_column_chunk_for_test(&encoded).expect("decode ALP floats");

        // Assert
        assert_eq!(codec, "alp");
        assert_eq!(decoded.len(), values.len());
        for (actual, expected) in decoded.iter().zip(values.iter()) {
            assert_eq!(
                actual.as_f64().expect("decoded float").to_bits(),
                expected.as_f64().expect("input float").to_bits()
            );
        }
    }

    #[test]
    fn should_keep_signed_zero_out_of_alp() {
        // Arrange
        let negative_zero = serde_json::Number::from_f64(-0.0).expect("negative zero");
        let values = vec![
            serde_json::Value::Number(negative_zero),
            serde_json::json!(1.25),
        ];

        // Act
        let encoded = encode("float", &values);
        let codec = column_chunk_codec_for_test(&encoded).expect("float codec");
        let decoded = decode_column_chunk_for_test(&encoded).expect("decode floats");

        // Assert
        assert_ne!(codec, "alp");
        assert_eq!(
            decoded[0].as_f64().expect("decoded float").to_bits(),
            (-0.0_f64).to_bits()
        );
    }

    #[test]
    fn should_fall_back_to_plain_when_savings_do_not_clear_the_threshold() {
        // Arrange
        let values = (0..64)
            .map(|value| serde_json::json!(format!("unique-{value:04}")))
            .collect::<Vec<_>>();

        // Act
        let encoded = encode("text", &values);
        let codec = column_chunk_codec_for_test(&encoded).expect("plain codec");

        // Assert
        assert_eq!(codec, "plain");
    }

    #[test]
    fn should_reduce_representative_compressible_bytes_by_at_least_twenty_five_percent() {
        // Arrange
        let values = (0..1_024)
            .map(|position| serde_json::json!(format!("status-{}", position % 4)))
            .collect::<Vec<_>>();

        // Act
        let selected = encode("text", &values);
        let plain =
            encode_plain_column_chunk_for_test("text", &values).expect("encode plain baseline");

        // Assert
        assert!(selected.len() <= plain.len());
        assert!(
            selected.len().saturating_mul(4) <= plain.len().saturating_mul(3),
            "selected={} plain={}",
            selected.len(),
            plain.len()
        );
    }

    #[test]
    fn should_validate_selected_dictionary_payload_before_partial_decode() {
        // Arrange
        let values = (0..512)
            .map(|position| serde_json::json!(format!("long-dictionary-value-{}", position % 4)))
            .collect::<Vec<_>>();
        let encoded = encode("text", &values);
        let selection = (0..values.len())
            .map(|position| matches!(position, 17 | 255 | 511))
            .collect::<Vec<_>>();

        // Act
        let decoded = decode_selected_column_chunk_for_test(&encoded, &selection)
            .expect("decode selected dictionary values");

        // Assert
        assert_eq!(decoded.len(), values.len());
        for (position, value) in decoded.iter().enumerate() {
            if selection[position] {
                assert_eq!(value, &values[position]);
            } else {
                assert!(value.is_null());
            }
        }
        for boundary in 0..encoded.len() {
            assert!(
                decode_selected_column_chunk_for_test(&encoded[..boundary], &selection).is_err(),
                "boundary {boundary} unexpectedly decoded"
            );
        }
    }

    #[test]
    fn should_reject_every_truncated_column_chunk_boundary() {
        // Arrange
        let encoded = encode(
            "bigint",
            &(0..300)
                .map(|value| serde_json::json!(value))
                .collect::<Vec<_>>(),
        );

        // Act
        let outcomes = (0..encoded.len())
            .map(|boundary| decode_column_chunk_for_test(&encoded[..boundary]))
            .collect::<Vec<_>>();

        // Assert
        for (boundary, outcome) in outcomes.into_iter().enumerate() {
            assert!(outcome.is_err(), "boundary {boundary} unexpectedly decoded");
        }
    }

    #[test]
    fn should_reject_unsupported_or_oversized_column_chunks() {
        // Arrange
        let encoded = encode("bigint", &[serde_json::json!(1), serde_json::json!(2)]);
        let mut unknown_codec = encoded.clone();
        unknown_codec[7] = u8::MAX;
        let mut excessive_count = encoded;
        excessive_count[10..14].copy_from_slice(&u32::MAX.to_le_bytes());

        // Act
        let codec_error = decode_column_chunk_for_test(&unknown_codec);
        let count_error = decode_column_chunk_for_test(&excessive_count);

        // Assert
        assert!(codec_error.is_err());
        assert!(count_error.is_err());
    }

    #[test]
    fn should_validate_bounded_row_id_chunk_roundtrips() {
        // Arrange
        let row_ids = vec![
            "row-0001".to_string(),
            "row-0002".to_string(),
            "row-0100".to_string(),
        ];

        // Act
        let encoded = encode_row_id_chunk_for_test(&row_ids).expect("encode row IDs");
        let decoded = decode_row_id_chunk_for_test(&encoded).expect("decode row IDs");

        // Assert
        assert_eq!(&encoded[..4], b"CBR2");
        assert_eq!(decoded, row_ids);
        for boundary in 0..encoded.len() {
            assert!(
                decode_row_id_chunk_for_test(&encoded[..boundary]).is_err(),
                "boundary {boundary} unexpectedly decoded"
            );
        }
    }

    #[test]
    fn should_validate_manifest_roundtrips_against_truncation_or_checksum_failure() {
        // Arrange
        let metadata = ColumnBatchMetadata {
            metadata_format_version: 2,
            summary_format_version: 2,
            manifest_revision: 9,
            next_segment_id: 0,
            collection: "postgres.public.events".to_string(),
            index_name: "events_column_idx".to_string(),
            schema_version: 7,
            built_generation: 11,
            source_row_count: 0,
            fields: vec!["tenant".to_string(), "amount".to_string()],
            segment_size: 128,
            segments: Vec::new(),
        };

        // Act
        let encoded =
            encode_column_batch_manifest_for_test(&metadata).expect("encode canonical manifest");
        let decoded =
            decode_column_batch_manifest_for_test(&encoded).expect("decode canonical manifest");

        // Assert
        assert_eq!(decoded, metadata);
        assert!(encoded.starts_with(b"CBM2"));
        for boundary in 0..encoded.len() {
            assert!(
                decode_column_batch_manifest_for_test(&encoded[..boundary]).is_err(),
                "accepted manifest truncation at boundary {boundary}"
            );
        }
        for position in [0, encoded.len() / 2, encoded.len() - 1] {
            let mut corrupt = encoded.clone();
            corrupt[position] ^= 0x40;
            assert!(
                decode_column_batch_manifest_for_test(&corrupt).is_err(),
                "accepted corrupt manifest byte at {position}"
            );
        }
        let mut trailing = encoded;
        trailing.push(0);
        assert!(decode_column_batch_manifest_for_test(&trailing).is_err());
    }
}

// Formerly tests/column_batch_incremental_maintenance.rs.
mod column_batch_incremental_maintenance {
    use cassie::app::Cassie;
    use cassie::catalog::{ColumnBatchMetadata, ColumnBatchSegmentMeta};
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::Value;
    use std::sync::{Arc, Barrier};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn segment_owns(segment: &ColumnBatchSegmentMeta, id: &str) -> bool {
        segment
            .row_id_start
            .as_deref()
            .is_some_and(|start| id >= start)
            && segment.row_id_end.as_deref().is_none_or(|end| id < end)
    }

    fn assert_single_segment_rewrite(
        before: &ColumnBatchMetadata,
        after: &ColumnBatchMetadata,
        touched_id: u64,
    ) {
        assert_eq!(after.segments.len(), before.segments.len());
        for before_segment in &before.segments {
            let after_segment = after
                .segments
                .iter()
                .find(|segment| segment.segment_id == before_segment.segment_id)
                .expect("segment remains present");
            if before_segment.segment_id == touched_id {
                assert_eq!(after_segment.revision, before_segment.revision + 1);
                assert_ne!(
                    after_segment.summary_checksum,
                    before_segment.summary_checksum
                );
            } else {
                assert_eq!(after_segment.revision, before_segment.revision);
                assert_eq!(
                    after_segment.field_chunks, before_segment.field_chunks,
                    "untouched segment chunks changed"
                );
                assert_eq!(
                    after_segment.row_ids, before_segment.row_ids,
                    "untouched row IDs changed"
                );
            }
        }
    }

    fn metric_delta(before: &serde_json::Value, after: &serde_json::Value, name: &str) -> u64 {
        after["column_batches"][name]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(before["column_batches"][name].as_u64().unwrap_or_default())
    }

    fn assert_median_split(before: &ColumnBatchMetadata, after: &ColumnBatchMetadata) {
        assert_eq!(after.segments.len(), 2);
        assert_eq!(after.segments[0].segment_id, before.segments[0].segment_id);
        assert_eq!(after.segments[0].revision, before.segments[0].revision + 1);
        assert_ne!(after.segments[1].segment_id, after.segments[0].segment_id);
        assert_eq!(after.segments[1].revision, 1);
        assert_eq!(
            after
                .segments
                .iter()
                .map(|segment| segment.row_count)
                .sum::<usize>(),
            5
        );
        assert_eq!(after.segments[0].row_id_end, after.segments[1].row_id_start);
    }

    #[test]
    fn should_rewrite_only_the_touched_segment_for_single_row_dml() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_incremental_single_row");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE incremental_single_row (status TEXT, amount INT)",
                    vec![],
                )
                .expect("create table");
            for amount in 0..8 {
                cassie
                    .midge
                    .put_document(
                        "incremental_single_row",
                        Some(format!("row-{amount:02}")),
                        serde_json::json!({ "status": "old", "amount": amount }),
                    )
                    .expect("seed document");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX incremental_single_row_idx ON incremental_single_row \
                 USING column (status, amount) WITH (segment_size = 2)",
                    vec![],
                )
                .expect("create column index");
            let before = cassie
                .midge
                .get_column_batch_metadata("incremental_single_row", "incremental_single_row_idx")
                .expect("load metadata")
                .expect("metadata exists");
            let touched_id = before
                .segments
                .iter()
                .find(|segment| segment_owns(segment, "row-03"))
                .expect("find touched segment")
                .segment_id;
            let metrics_before = cassie.metrics();

            // Act
            cassie
                .midge
                .put_document(
                    "incremental_single_row",
                    Some("row-03".to_string()),
                    serde_json::json!({ "status": "updated", "amount": 30 }),
                )
                .expect("update one document");
            let after_update = cassie
                .midge
                .get_column_batch_metadata("incremental_single_row", "incremental_single_row_idx")
                .expect("load updated metadata")
                .expect("updated metadata exists");
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT amount FROM incremental_single_row \
                 WHERE status = 'updated' ORDER BY amount",
                    vec![],
                )
                .expect("query updated column batch");
            let metrics_after = cassie.metrics();
            let persisted_chunk_count = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"")
                .expect("scan persisted column chunks")
                .into_iter()
                .filter(|(_, value)| {
                    value.starts_with(b"CBM2")
                        || value.starts_with(b"CBR2")
                        || value.starts_with(b"CBC2")
                })
                .count();

            // Assert
            assert_single_segment_rewrite(&before, &after_update, touched_id);
            assert_eq!(result.rows, vec![vec![Value::Int64(30)]]);
            assert_eq!(persisted_chunk_count, 13);
            assert_eq!(
                metric_delta(&metrics_before, &metrics_after, "segment_rewrites"),
                1
            );
            assert_eq!(
                metric_delta(&metrics_before, &metrics_after, "orphan_revisions_cleaned"),
                3
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_split_an_overflowing_range_at_the_median() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_incremental_split");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE incremental_split (label TEXT, amount INT)",
                    vec![],
                )
                .expect("create table");
            for (id, amount) in [("row-00", 0), ("row-99", 99)] {
                cassie
                    .midge
                    .put_document(
                        "incremental_split",
                        Some(id.to_string()),
                        serde_json::json!({ "label": id, "amount": amount }),
                    )
                    .expect("seed document");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX incremental_split_idx ON incremental_split \
                 USING column (label, amount) WITH (segment_size = 2)",
                    vec![],
                )
                .expect("create column index");
            for (id, amount) in [("row-10", 10), ("row-20", 20)] {
                cassie
                    .midge
                    .put_document(
                        "incremental_split",
                        Some(id.to_string()),
                        serde_json::json!({ "label": id, "amount": amount }),
                    )
                    .expect("grow segment within limit");
            }
            let before_split = cassie
                .midge
                .get_column_batch_metadata("incremental_split", "incremental_split_idx")
                .expect("load metadata")
                .expect("metadata exists");
            assert_eq!(before_split.segments.len(), 1);
            assert_eq!(before_split.segments[0].row_count, 4);
            let metrics_before = cassie.metrics();

            // Act
            cassie
                .midge
                .put_document(
                    "incremental_split",
                    Some("row-30".to_string()),
                    serde_json::json!({ "label": "row-30", "amount": 30 }),
                )
                .expect("insert split row");
            let after_split = cassie
                .midge
                .get_column_batch_metadata("incremental_split", "incremental_split_idx")
                .expect("load split metadata")
                .expect("split metadata exists");
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT amount FROM incremental_split ORDER BY amount",
                    vec![],
                )
                .expect("query split segments");
            let metrics_after = cassie.metrics();

            // Assert
            assert_median_split(&before_split, &after_split);
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::Int64(0)],
                    vec![Value::Int64(10)],
                    vec![Value::Int64(20)],
                    vec![Value::Int64(30)],
                    vec![Value::Int64(99)],
                ]
            );
            assert_eq!(
                metric_delta(&metrics_before, &metrics_after, "segment_rewrites"),
                2
            );
            assert_eq!(
                metric_delta(&metrics_before, &metrics_after, "segment_splits"),
                1
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rewrite_one_segment_for_actual_deletes_only() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_incremental_delete");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE incremental_delete (label TEXT, amount INT)",
                    vec![],
                )
                .expect("create table");
            for amount in 0..8 {
                cassie
                    .midge
                    .put_document(
                        "incremental_delete",
                        Some(format!("row-{amount:02}")),
                        serde_json::json!({ "label": format!("value-{amount}"), "amount": amount }),
                    )
                    .expect("seed document");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX incremental_delete_idx ON incremental_delete \
                 USING column (label, amount) WITH (segment_size = 2)",
                    vec![],
                )
                .expect("create column index");
            let before = cassie
                .midge
                .get_column_batch_metadata("incremental_delete", "incremental_delete_idx")
                .expect("load metadata")
                .expect("metadata exists");
            let touched_id = before
                .segments
                .iter()
                .find(|segment| segment_owns(segment, "row-03"))
                .expect("find touched segment")
                .segment_id;
            let metrics_before = cassie.metrics();

            // Act
            let missing_deleted = cassie
                .midge
                .delete_document("incremental_delete", "missing")
                .expect("delete missing document");
            let metrics_after_missing = cassie.metrics();
            let actual_deleted = cassie
                .midge
                .delete_document("incremental_delete", "row-03")
                .expect("delete actual document");
            let after = cassie
                .midge
                .get_column_batch_metadata("incremental_delete", "incremental_delete_idx")
                .expect("load updated metadata")
                .expect("updated metadata exists");
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT amount FROM incremental_delete ORDER BY amount",
                    vec![],
                )
                .expect("query after delete");
            let metrics_after = cassie.metrics();

            // Assert
            assert!(!missing_deleted);
            assert!(actual_deleted);
            assert_eq!(
                metric_delta(&metrics_before, &metrics_after_missing, "segment_rewrites"),
                0
            );
            assert_single_segment_rewrite(&before, &after, touched_id);
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::Int64(0)],
                    vec![Value::Int64(1)],
                    vec![Value::Int64(2)],
                    vec![Value::Int64(4)],
                    vec![Value::Int64(5)],
                    vec![Value::Int64(6)],
                    vec![Value::Int64(7)],
                ]
            );
            assert_eq!(
                metric_delta(&metrics_after_missing, &metrics_after, "segment_rewrites"),
                1
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_concurrent_readers_on_complete_published_generations() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_incremental_concurrent_readers");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE incremental_concurrent (label TEXT, amount INT)",
                vec![],
            )
            .expect("create table");
        for amount in 0..32 {
            cassie
                .midge
                .put_document(
                    "incremental_concurrent",
                    Some(format!("row-{amount:02}")),
                    serde_json::json!({
                        "label": if amount == 3 { "target" } else { "other" },
                        "amount": amount
                    }),
                )
                .expect("seed document");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX incremental_concurrent_idx ON incremental_concurrent \
             USING column (label, amount) WITH (segment_size = 4)",
                vec![],
            )
            .expect("create column index");
        let start = Arc::new(Barrier::new(6));
        let mut readers = Vec::new();
        for reader_id in 0..4 {
            let reader_cassie = Arc::clone(&cassie);
            let reader_start = Arc::clone(&start);
            readers.push(std::thread::spawn(move || {
                let reader_name = format!("reader-{reader_id}");
                let reader = reader_cassie.create_session(&reader_name, None);
                reader_start.wait();
                for _ in 0..100 {
                    let result = reader_cassie
                        .execute_sql(
                            &reader,
                            "SELECT amount FROM incremental_concurrent WHERE label = 'target'",
                            vec![],
                        )
                        .expect("concurrent encoded read");
                    assert_eq!(result.rows.len(), 1);
                    assert!(matches!(result.rows[0].as_slice(), [Value::Int64(3 | 30)]));
                }
            }));
        }
        let writer_cassie = Arc::clone(&cassie);
        let writer_start = Arc::clone(&start);
        let writer = std::thread::spawn(move || {
            writer_start.wait();
            for iteration in 0..100 {
                writer_cassie
                    .midge
                    .put_document(
                        "incremental_concurrent",
                        Some("row-03".to_string()),
                        serde_json::json!({
                            "label": "target",
                            "amount": if iteration % 2 == 0 { 30 } else { 3 }
                        }),
                    )
                    .expect("concurrent incremental update");
            }
        });

        // Act
        start.wait();
        writer.join().expect("writer thread");
        for reader in readers {
            reader.join().expect("reader thread");
        }

        // Assert
        let final_result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM incremental_concurrent WHERE label = 'target'",
                vec![],
            )
            .expect("final encoded read");
        assert_eq!(final_result.rows, vec![vec![Value::Int64(3)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_compact_excess_sparse_ranges_without_changing_query_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_incremental_compaction");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE incremental_compaction (amount INT)",
                    vec![],
                )
                .expect("create table");
            for amount in 0..16 {
                cassie
                    .midge
                    .put_document(
                        "incremental_compaction",
                        Some(format!("row-{amount:02}")),
                        serde_json::json!({ "amount": amount }),
                    )
                    .expect("seed document");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX incremental_compaction_idx ON incremental_compaction \
                 USING column (amount) WITH (segment_size = 4)",
                    vec![],
                )
                .expect("create column index");
            let metrics_before = cassie.metrics();

            // Act
            for deleted in [0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14] {
                assert!(cassie
                    .midge
                    .delete_document("incremental_compaction", &format!("row-{deleted:02}"))
                    .expect("delete sparse document"));
            }
            let metadata = cassie
                .midge
                .get_column_batch_metadata("incremental_compaction", "incremental_compaction_idx")
                .expect("load compacted metadata")
                .expect("metadata exists");
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT amount FROM incremental_compaction ORDER BY amount",
                    vec![],
                )
                .expect("query compacted ranges");
            let metrics_after = cassie.metrics();

            // Assert
            assert_eq!(metadata.segments.len(), 1);
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::Int64(3)],
                    vec![Value::Int64(7)],
                    vec![Value::Int64(11)],
                    vec![Value::Int64(15)],
                ]
            );
            assert!(
                metric_delta(&metrics_before, &metrics_after, "compactions") >= 1,
                "sparse ranges did not trigger copy-on-write compaction"
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/column_batch_resilience.rs.
mod column_batch_resilience {
    use cassie::app::{Cassie, CassieSession};
    use cassie::catalog::ColumnBatchMetadata;
    use cassie::midge::adapter::{
        decode_column_batch_manifest_for_test, encode_column_batch_manifest_for_test,
        set_column_batch_maintenance_failure_point, StorageFamily,
    };
    use cassie::types::Value;
    use cntryl_midge::{TransactionMode, WriteOptions};
    use sha2::{Digest, Sha256};

    use super::support_sql as support;
    use support::{canonical_test_collection, canonical_test_index, data_dir, use_local_storage};

    use super::support_failpoints as failpoints;
    use failpoints::COLUMN_BATCH_FAILPOINT_GUARD;

    struct AmountFixture {
        test_guard: std::sync::MutexGuard<'static, ()>,
        path: String,
        cassie: Cassie,
        session: CassieSession,
        collection: String,
        index: String,
    }

    fn amount_fixture(label: &str, table: &str, values: &[Value]) -> AmountFixture {
        let test_guard = COLUMN_BATCH_FAILPOINT_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {table} (amount BIGINT)"),
                vec![],
            )
            .expect("create table");
        for value in values {
            cassie
                .execute_sql(
                    &session,
                    &format!("INSERT INTO {table} (amount) VALUES ($1)"),
                    vec![value.clone()],
                )
                .expect("insert amount");
        }
        let index_name = format!("{table}_column_idx");
        cassie
            .execute_sql(
                &session,
                &format!(
                "CREATE INDEX {index_name} ON {table} USING column (amount) WITH (segment_size = 1)"
            ),
                vec![],
            )
            .expect("create column index");
        let collection = canonical_test_collection(&cassie, table);
        let index = canonical_test_index(&cassie, &collection, &index_name);
        AmountFixture {
            test_guard,
            path,
            cassie,
            session,
            collection,
            index,
        }
    }

    fn ordered_numeric_fixture(
        label: &str,
        table: &str,
        column_type: &str,
        values: &[serde_json::Value],
        segment_size: usize,
    ) -> AmountFixture {
        let test_guard = COLUMN_BATCH_FAILPOINT_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {table} (amount {column_type})"),
                vec![],
            )
            .expect("create table");
        let collection = canonical_test_collection(&cassie, table);
        for (position, value) in values.iter().enumerate() {
            cassie
                .midge
                .put_document(
                    &collection,
                    Some(format!("row-{position:04}")),
                    serde_json::json!({ "amount": value }),
                )
                .expect("insert ordered amount");
        }
        let index_name = format!("{table}_column_idx");
        cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE INDEX {index_name} ON {table} USING column (amount) WITH (segment_size = {segment_size})"
            ),
            vec![],
        )
        .expect("create column index");
        let index = canonical_test_index(&cassie, &collection, &index_name);
        AmountFixture {
            test_guard,
            path,
            cassie,
            session,
            collection,
            index,
        }
    }

    fn metadata_entry(fixture: &AmountFixture) -> (Vec<u8>, ColumnBatchMetadata) {
        fixture
            .cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan data")
            .into_iter()
            .find_map(|(key, raw)| {
                raw.starts_with(b"CBM2")
                    .then(|| decode_column_batch_manifest_for_test(&raw).ok())
                    .flatten()
                    .map(|metadata| (key, metadata))
            })
            .expect("column batch metadata")
    }

    fn write_metadata(fixture: &AmountFixture, key: Vec<u8>, metadata: &ColumnBatchMetadata) {
        write_raw_metadata(
            fixture,
            key,
            encode_column_batch_manifest_for_test(metadata).expect("encode metadata"),
        );
    }

    fn write_raw_metadata(fixture: &AmountFixture, key: Vec<u8>, metadata: Vec<u8>) {
        let mut tx = fixture
            .cassie
            .midge
            .data_tx(TransactionMode::ReadWrite)
            .expect("open data transaction");
        tx.put(key, metadata, None).expect("write metadata");
        tx.commit(WriteOptions::sync())
            .expect("commit metadata mutation");
    }

    fn resign_manifest(mut raw: Vec<u8>) -> Vec<u8> {
        raw.truncate(raw.len().saturating_sub(32));
        let checksum = Sha256::digest(raw.as_slice());
        raw.extend_from_slice(checksum.as_slice());
        raw
    }

    fn sum_amount(fixture: &AmountFixture, table: &str) -> Result<Vec<Vec<Value>>, String> {
        sum_amount_as(fixture, table, "total")
    }

    fn sum_amount_as(
        fixture: &AmountFixture,
        table: &str,
        alias: &str,
    ) -> Result<Vec<Vec<Value>>, String> {
        fixture
            .cassie
            .execute_sql(
                &fixture.session,
                &format!("SELECT SUM(amount) AS {alias} FROM {table}"),
                vec![],
            )
            .map(|result| result.rows)
            .map_err(|error| error.to_string())
    }

    fn restart_fixture(fixture: AmountFixture) -> AmountFixture {
        let AmountFixture {
            test_guard,
            path,
            cassie,
            session,
            collection,
            index,
        } = fixture;
        drop(session);
        drop(cassie);
        let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        cassie.startup().expect("reconcile column batches");
        let session = cassie.create_session("tester", None);
        AmountFixture {
            test_guard,
            path,
            cassie,
            session,
            collection,
            index,
        }
    }

    #[test]
    fn should_preserve_integer_sum_above_two_to_the_fifty_third() {
        // Arrange
        let _test_guard = COLUMN_BATCH_FAILPOINT_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use_local_storage();
        let path = data_dir("column_batch_large_integer_sum");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_large_integer_sum (amount BIGINT)",
                vec![],
            )
            .expect("create table");
        for amount in [9_007_199_254_740_993_i64, 2] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO column_batch_large_integer_sum (amount) VALUES ($1)",
                    vec![Value::Int64(amount)],
                )
                .expect("insert amount");
        }
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX column_batch_large_integer_sum_idx ON column_batch_large_integer_sum USING column (amount) WITH (segment_size = 1)",
            vec![],
        )
        .expect("create column index");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT SUM(amount) AS total FROM column_batch_large_integer_sum",
                vec![],
            )
            .expect("aggregate large integers");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(9_007_199_254_740_995)]]);
        assert_eq!(cassie.metrics()["aggregate_acceleration"]["scans"], 1);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reconcile_failed_refresh_debt_after_restart() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_failed_refresh",
            "column_batch_failed_refresh",
            &[Value::Int64(2)],
        );
        set_column_batch_maintenance_failure_point(true);
        fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "INSERT INTO column_batch_failed_refresh (amount) VALUES (8)",
                Vec::new(),
            )
            .expect("row write remains durable");
        let source_rows = fixture
            .cassie
            .midge
            .scan_documents(&fixture.collection)
            .expect("scan source rows");
        assert_eq!(source_rows.len(), 2, "source rows: {source_rows:?}");
        assert!(fixture
            .cassie
            .midge
            .has_column_batch_maintenance_debt(&fixture.collection)
            .expect("read maintenance debt"));

        // Act
        let fallback = sum_amount(&fixture, "column_batch_failed_refresh")
            .expect("fall back to exact aggregate");
        let fallback_metrics = fixture.cassie.metrics();
        let restarted = restart_fixture(fixture);
        let recovered = sum_amount(&restarted, "column_batch_failed_refresh")
            .expect("aggregate rebuilt summaries");
        let recovered_metrics = restarted.cassie.metrics();

        // Assert
        assert_eq!(fallback, vec![vec![Value::Int64(10)]]);
        assert_eq!(fallback_metrics["aggregate_acceleration"]["scans"], 0);
        assert_eq!(
            fallback_metrics["aggregate_acceleration"]["row_blob_fallbacks"],
            1
        );
        assert_eq!(
            fallback_metrics["column_batches"]["last_fallback_reason"],
            "maintenance_pending"
        );
        assert_eq!(recovered, fallback);
        assert_eq!(recovered_metrics["aggregate_acceleration"]["scans"], 1);
        assert!(!restarted
            .cassie
            .midge
            .has_column_batch_maintenance_debt(&restarted.collection)
            .expect("maintenance debt cleared"));

        let _ = std::fs::remove_dir_all(&restarted.path);
    }

    #[test]
    fn should_reconcile_existing_maintenance_debt_when_publishing_column_index() {
        // Arrange
        let _test_guard = COLUMN_BATCH_FAILPOINT_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use_local_storage();
        let path = data_dir("column_batch_publication_debt");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_publication_debt (amount BIGINT)",
                vec![],
            )
            .expect("create table");
        let collection = canonical_test_collection(&cassie, "column_batch_publication_debt");
        set_column_batch_maintenance_failure_point(true);
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_publication_debt (amount) VALUES (8)",
                vec![],
            )
            .expect("row write remains durable");
        assert!(cassie
            .midge
            .has_column_batch_maintenance_debt(&collection)
            .expect("read maintenance debt"));

        // Act
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX column_batch_publication_debt_idx ON column_batch_publication_debt USING column (amount) WITH (segment_size = 1)",
            vec![],
        )
        .expect("publish column index");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT SUM(amount) AS total FROM column_batch_publication_debt",
                vec![],
            )
            .expect("read published column index");

        // Assert
        assert!(!cassie
            .midge
            .has_column_batch_maintenance_debt(&collection)
            .expect("maintenance debt cleared"));
        assert_eq!(result.rows, vec![vec![Value::Int64(8)]]);
        assert_eq!(cassie.metrics()["aggregate_acceleration"]["scans"], 1);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_generation_mismatch_after_restart() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_generation_mismatch",
            "column_batch_generation_mismatch",
            &[Value::Int64(4), Value::Int64(6)],
        );
        let (key, mut metadata) = metadata_entry(&fixture);
        metadata.built_generation = metadata.built_generation.saturating_add(1);
        write_metadata(&fixture, key, &metadata);

        // Act
        let fallback = sum_amount(&fixture, "column_batch_generation_mismatch")
            .expect("fall back to exact aggregate");
        let fallback_metrics = fixture.cassie.metrics();
        let restarted = restart_fixture(fixture);
        let recovered = sum_amount(&restarted, "column_batch_generation_mismatch")
            .expect("aggregate rebuilt summaries");
        let recovered_metrics = restarted.cassie.metrics();

        // Assert
        assert_eq!(fallback, vec![vec![Value::Int64(10)]]);
        assert_eq!(fallback_metrics["aggregate_acceleration"]["scans"], 0);
        assert_eq!(
            fallback_metrics["column_batches"]["last_fallback_reason"],
            "generation_mismatch"
        );
        assert_eq!(recovered, fallback);
        assert_eq!(recovered_metrics["aggregate_acceleration"]["scans"], 1);
        let repaired = restarted
            .cassie
            .midge
            .get_column_batch_metadata(&restarted.collection, &restarted.index)
            .expect("read repaired metadata")
            .expect("repaired metadata");
        assert_eq!(
            repaired.built_generation,
            restarted
                .cassie
                .midge
                .collection_generation(&restarted.collection)
                .expect("current generation")
        );

        let _ = std::fs::remove_dir_all(&restarted.path);
    }

    #[test]
    fn should_report_checked_integer_overflow_without_publishing_acceleration() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_integer_overflow",
            "column_batch_integer_overflow",
            &[Value::Int64(i64::MAX), Value::Int64(1)],
        );

        // Act
        let accelerated_error = sum_amount(&fixture, "column_batch_integer_overflow")
            .expect_err("accelerated SUM must report overflow");
        let (key, mut metadata) = metadata_entry(&fixture);
        metadata.built_generation = metadata.built_generation.saturating_add(1);
        write_metadata(&fixture, key, &metadata);
        let exact_error = sum_amount(&fixture, "column_batch_integer_overflow")
            .expect_err("exact SUM must report overflow");
        let metrics = fixture.cassie.metrics();

        // Assert
        assert!(accelerated_error.contains("aggregate integer overflow"));
        assert_eq!(exact_error, accelerated_error);
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
        assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);

        let _ = std::fs::remove_dir_all(&fixture.path);
    }

    #[test]
    fn should_detect_checked_integer_overflow_across_segment_boundaries() {
        // Arrange
        let fixture = ordered_numeric_fixture(
            "column_batch_cross_segment_overflow",
            "column_batch_cross_segment_overflow",
            "BIGINT",
            &[
                serde_json::json!(i64::MAX),
                serde_json::json!(0),
                serde_json::json!(1),
                serde_json::json!(-1),
            ],
            2,
        );

        // Act
        let accelerated = sum_amount(&fixture, "column_batch_cross_segment_overflow")
            .expect_err("summary fold must detect row-order overflow");
        let metrics = fixture.cassie.metrics();
        fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_cross_segment_overflow_column_idx ON column_batch_cross_segment_overflow",
            vec![],
        )
        .expect("drop column index");
        let exact = sum_amount_as(
            &fixture,
            "column_batch_cross_segment_overflow",
            "exact_total",
        )
        .expect_err("row fold must detect overflow");

        // Assert
        assert_eq!(accelerated, exact);
        assert!(accelerated.contains("aggregate integer overflow"));
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);

        let _ = std::fs::remove_dir_all(&fixture.path);
    }

    #[test]
    fn should_not_invent_checked_integer_overflow_inside_a_segment() {
        // Arrange
        let fixture = ordered_numeric_fixture(
            "column_batch_cross_segment_safe_sum",
            "column_batch_cross_segment_safe_sum",
            "BIGINT",
            &[
                serde_json::json!(-1),
                serde_json::json!(0),
                serde_json::json!(i64::MAX),
                serde_json::json!(1),
            ],
            2,
        );

        // Act
        let accelerated = sum_amount(&fixture, "column_batch_cross_segment_safe_sum")
            .expect("incoming sum keeps the segment fold in range");
        let metrics = fixture.cassie.metrics();
        fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_cross_segment_safe_sum_column_idx ON column_batch_cross_segment_safe_sum",
            vec![],
        )
        .expect("drop column index");
        let exact = sum_amount_as(
            &fixture,
            "column_batch_cross_segment_safe_sum",
            "exact_total",
        )
        .expect("exact row fold");

        // Assert
        assert_eq!(accelerated, vec![vec![Value::Int64(i64::MAX)]]);
        assert_eq!(exact, accelerated);
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 1);

        let _ = std::fs::remove_dir_all(&fixture.path);
    }

    #[test]
    fn should_fallback_when_floating_summaries_cannot_preserve_row_order() {
        // Arrange
        let fixture = ordered_numeric_fixture(
            "column_batch_float_row_order",
            "column_batch_float_row_order",
            "FLOAT",
            &[
                serde_json::json!(10_000_000_000_000_000.0),
                serde_json::json!(0.0),
                serde_json::json!(-10_000_000_000_000_000.0),
                serde_json::json!(1.0),
            ],
            2,
        );

        // Act
        let fallback = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS fallback_sum, AVG(amount) AS fallback_avg FROM column_batch_float_row_order",
            vec![],
        )
        .expect("fall back to row-order floating aggregates");
        let metrics = fixture.cassie.metrics();
        fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_float_row_order_column_idx ON column_batch_float_row_order",
            vec![],
        )
        .expect("drop column index");
        let exact = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS exact_sum, AVG(amount) AS exact_avg FROM column_batch_float_row_order",
            vec![],
        )
        .expect("run exact floating aggregates");

        // Assert
        assert_eq!(
            fallback.rows,
            vec![vec![Value::Float64(1.0), Value::Float64(0.25)]]
        );
        assert_eq!(exact.rows, fallback.rows);
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
        assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
        assert_eq!(
            metrics["column_batches"]["last_fallback_reason"],
            "numeric_summary_requires_rows"
        );

        let _ = std::fs::remove_dir_all(&fixture.path);
    }

    #[test]
    fn should_rebuild_old_metadata_format_after_restart() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_metadata_format",
            "column_batch_metadata_format",
            &[Value::Int64(3), Value::Int64(7)],
        );
        let (key, _) = metadata_entry(&fixture);
        write_raw_metadata(&fixture, key, b"CCB1-derived-v1".to_vec());

        // Act
        let fallback = sum_amount(&fixture, "column_batch_metadata_format")
            .expect("old metadata format falls back");
        let metrics = fixture.cassie.metrics();
        let repaired = restart_fixture(fixture);
        let recovered =
            sum_amount(&repaired, "column_batch_metadata_format").expect("metadata format rebuilt");

        // Assert
        assert_eq!(fallback, vec![vec![Value::Int64(10)]]);
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
        assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
        assert_eq!(
            metrics["column_batches"]["last_fallback_reason"],
            "metadata_format_mismatch"
        );
        assert_eq!(recovered, fallback);
        assert_eq!(
            repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
            1
        );

        let _ = std::fs::remove_dir_all(&repaired.path);
    }

    #[test]
    fn should_reconcile_invalid_summary_formats_after_restart() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_summary_recovery",
            "column_batch_summary_recovery",
            &[Value::Int64(3), Value::Int64(7)],
        );
        let (key, metadata) = metadata_entry(&fixture);
        let mut raw =
            encode_column_batch_manifest_for_test(&metadata).expect("encode current manifest");
        raw[6..8].copy_from_slice(&0_u16.to_le_bytes());
        write_raw_metadata(&fixture, key, resign_manifest(raw));

        // Act
        let old_format = sum_amount_as(&fixture, "column_batch_summary_recovery", "old_total")
            .expect("old format falls back");
        let old_metrics = fixture.cassie.metrics();
        let repaired = restart_fixture(fixture);
        let repaired_rows =
            sum_amount_as(&repaired, "column_batch_summary_recovery", "repaired_total")
                .expect("old format rebuilt");
        let (key, _) = metadata_entry(&repaired);
        let mut malformed = b"CBM2".to_vec();
        malformed.extend_from_slice(&2_u16.to_le_bytes());
        malformed.extend_from_slice(&2_u16.to_le_bytes());
        malformed.extend_from_slice(&[0_u8; 8]);
        malformed.extend_from_slice(Sha256::digest(malformed.as_slice()).as_slice());
        write_raw_metadata(&repaired, key, malformed);
        let malformed = sum_amount_as(
            &repaired,
            "column_batch_summary_recovery",
            "malformed_total",
        )
        .expect("malformed summary falls back");
        let malformed_metrics = repaired.cassie.metrics();
        let repaired = restart_fixture(repaired);
        let (key, mut metadata) = metadata_entry(&repaired);
        metadata.segments[0]
            .summaries
            .get_mut("amount")
            .expect("amount summary")
            .non_null_count = 99;
        write_metadata(&repaired, key, &metadata);
        let bad_checksum =
            sum_amount_as(&repaired, "column_batch_summary_recovery", "checksum_total")
                .expect("inconsistent summary falls back");
        let checksum_metrics = repaired.cassie.metrics();
        let repaired = restart_fixture(repaired);
        let final_rows = sum_amount_as(&repaired, "column_batch_summary_recovery", "final_total")
            .expect("inconsistent summary rebuilt");

        // Assert
        let expected = vec![vec![Value::Int64(10)]];
        assert_eq!(old_format, expected);
        assert_eq!(
            old_metrics["column_batches"]["last_fallback_reason"],
            "summary_format_mismatch"
        );
        assert_eq!(repaired_rows, expected);
        assert_eq!(malformed, expected);
        assert_eq!(
            malformed_metrics["column_batches"]["last_fallback_reason"],
            "invalid_metadata"
        );
        assert_eq!(bad_checksum, expected);
        assert_eq!(
            checksum_metrics["column_batches"]["last_fallback_reason"],
            "summary_checksum_mismatch"
        );
        assert_eq!(final_rows, expected);
        assert_eq!(
            repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
            1
        );

        let _ = std::fs::remove_dir_all(&repaired.path);
    }

    #[test]
    fn should_rebuild_source_count_mismatch_after_restart() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_source_count",
            "column_batch_source_count",
            &[Value::Int64(2), Value::Int64(5)],
        );
        let (key, mut metadata) = metadata_entry(&fixture);
        metadata.source_row_count = 3;
        write_metadata(&fixture, key, &metadata);

        // Act
        let fallback = sum_amount(&fixture, "column_batch_source_count")
            .expect("source count mismatch falls back");
        let metrics = fixture.cassie.metrics();
        let repaired = restart_fixture(fixture);
        let recovered =
            sum_amount(&repaired, "column_batch_source_count").expect("source count rebuilt");

        // Assert
        assert_eq!(fallback, vec![vec![Value::Int64(7)]]);
        assert_eq!(
            metrics["column_batches"]["last_fallback_reason"],
            "source_row_count_mismatch"
        );
        assert_eq!(recovered, fallback);
        assert_eq!(
            repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
            1
        );

        let _ = std::fs::remove_dir_all(&repaired.path);
    }

    #[test]
    fn should_fallback_when_any_segment_is_missing_and_rebuild_after_restart() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_missing_segment",
            "column_batch_missing_segment",
            &[Value::Int64(1), Value::Int64(2), Value::Int64(3)],
        );
        let segment_key = fixture
            .cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan column segments")
            .into_iter()
            .find_map(|(key, value)| value.starts_with(b"CBR2").then_some(key))
            .expect("persisted segment");
        fixture
            .cassie
            .midge
            .raw_delete(StorageFamily::Data, &segment_key)
            .expect("remove segment");

        // Act
        let fallback = sum_amount(&fixture, "column_batch_missing_segment")
            .expect("missing segment falls back");
        let metrics = fixture.cassie.metrics();
        let repaired = restart_fixture(fixture);
        let recovered =
            sum_amount(&repaired, "column_batch_missing_segment").expect("missing segment rebuilt");

        // Assert
        assert_eq!(fallback, vec![vec![Value::Int64(6)]]);
        assert_eq!(
            metrics["column_batches"]["last_fallback_reason"],
            "segment_missing"
        );
        assert_eq!(recovered, fallback);
        assert_eq!(
            repaired.cassie.metrics()["aggregate_acceleration"]["scans"],
            1
        );

        let _ = std::fs::remove_dir_all(&repaired.path);
    }

    #[test]
    fn should_preserve_all_null_aggregate_semantics() {
        // Arrange
        let fixture = amount_fixture(
            "column_batch_all_null",
            "column_batch_all_null",
            &[Value::Null, Value::Null],
        );

        // Act
        let result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COUNT(*) AS rows, COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM column_batch_all_null",
            vec![],
        )
        .expect("aggregate null rows");

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Int64(2),
                Value::Int64(0),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ]]
        );
        assert_eq!(
            fixture.cassie.metrics()["aggregate_acceleration"]["scans"],
            1
        );

        let _ = std::fs::remove_dir_all(&fixture.path);
    }

    #[test]
    fn should_preserve_zero_row_aggregate_semantics() {
        // Arrange
        let fixture = amount_fixture("column_batch_zero_rows", "column_batch_zero_rows", &[]);

        // Act
        let result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COUNT(*) AS rows, COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM column_batch_zero_rows",
            vec![],
        )
        .expect("aggregate empty table");
        let metrics = fixture.cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Int64(0),
                Value::Int64(0),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ]]
        );
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 1);
        assert_eq!(metrics["aggregate_acceleration"]["accelerated_segments"], 0);

        let _ = std::fs::remove_dir_all(&fixture.path);
    }

    #[test]
    fn should_preserve_mixed_numeric_summary_semantics() {
        // Arrange
        let fixture = ordered_numeric_fixture(
            "column_batch_mixed_numerics",
            "column_batch_mixed_numerics",
            "JSON",
            &[
                serde_json::json!(2),
                serde_json::json!(1.5),
                serde_json::json!(-1),
                serde_json::Value::Null,
            ],
            8,
        );

        // Act
        let accelerated = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS accelerated_sum, AVG(amount) AS accelerated_avg, MIN(amount) AS accelerated_min, MAX(amount) AS accelerated_max FROM column_batch_mixed_numerics",
            vec![],
        )
        .expect("aggregate mixed numeric summary");
        let accelerated_metrics = fixture.cassie.metrics();
        fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "DROP INDEX column_batch_mixed_numerics_column_idx ON column_batch_mixed_numerics",
                vec![],
            )
            .expect("drop column index");
        let exact = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT SUM(amount) AS exact_sum, AVG(amount) AS exact_avg, MIN(amount) AS exact_min, MAX(amount) AS exact_max FROM column_batch_mixed_numerics",
            vec![],
        )
        .expect("aggregate mixed numerics from rows");

        // Assert
        assert_eq!(
            accelerated.rows,
            vec![vec![
                Value::Float64(2.5),
                Value::Float64(2.5 / 3.0),
                Value::Int64(-1),
                Value::Int64(2),
            ]]
        );
        assert_eq!(exact.rows, accelerated.rows);
        assert_eq!(accelerated_metrics["aggregate_acceleration"]["scans"], 1);

        let _ = std::fs::remove_dir_all(&fixture.path);
    }

    #[test]
    fn should_fallback_when_typed_minmax_cannot_match_row_comparison() {
        // Arrange
        let fixture = ordered_numeric_fixture(
            "column_batch_typed_vector_minmax",
            "column_batch_typed_vector_minmax",
            "VECTOR(2)",
            &[
                serde_json::json!([1.0, 2.0]),
                serde_json::json!([0.0, 10.0]),
            ],
            2,
        );

        // Act
        let accelerated = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT MIN(amount) AS accelerated_min, MAX(amount) AS accelerated_max FROM column_batch_typed_vector_minmax",
            vec![],
        )
        .expect("aggregate typed vector summaries");
        let metrics = fixture.cassie.metrics();
        fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "DROP INDEX column_batch_typed_vector_minmax_column_idx ON column_batch_typed_vector_minmax",
            vec![],
        )
        .expect("drop column index");
        let exact = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT MIN(amount) AS exact_min, MAX(amount) AS exact_max FROM column_batch_typed_vector_minmax",
            vec![],
        )
        .expect("aggregate vectors from rows");

        // Assert
        assert_eq!(
            accelerated.rows,
            vec![vec![
                Value::String("[0,10]".to_string()),
                Value::String("[1,2]".to_string()),
            ]]
        );
        assert_eq!(exact.rows, accelerated.rows);
        assert_eq!(metrics["aggregate_acceleration"]["scans"], 0);
        assert_eq!(metrics["aggregate_acceleration"]["row_blob_fallbacks"], 1);
        assert_eq!(
            metrics["column_batches"]["last_fallback_reason"],
            "typed_summary_requires_rows"
        );

        let _ = std::fs::remove_dir_all(&fixture.path);
    }
}

// Formerly tests/column_batches.rs.
mod column_batches {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::catalog::IndexKind;
    use cassie::midge::adapter::StorageFamily;
    use cassie::sql::ast::QueryStatement;
    use cassie::sql::parse_statement;
    use cassie::types::Value;
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_parse_column_index_with_segment_size() {
        // Arrange
        let sql =
        "CREATE INDEX idx_docs_column ON docs USING column (title, body) WITH (segment_size = 2)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.name, "idx_docs_column");
        assert_eq!(
            statement.fields,
            vec!["title".to_string(), "body".to_string()]
        );
        assert_eq!(statement.kind, IndexKind::Column);
        assert_eq!(
            statement.options.get("segment_size"),
            Some(&"2".to_string())
        );
    }

    #[test]
    fn should_read_covered_projection_from_column_batch_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_projection");
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
                "CREATE TABLE column_batch_projection (title TEXT, body TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_projection (title, body, score) VALUES ('alpha', 'one', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_projection (title, score) VALUES ('beta', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_projection ON column_batch_projection USING column (title, body) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_projection WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title, body FROM column_batch_projection WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![
            Value::String("alpha".to_string()),
            Value::String("one".to_string()),
        ]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("column_batch_index=idx_column_batch_projection"));
        assert!(plan.contains("encoded_execution=true"));
        assert!(plan.contains(
            "automatic_codec_policy=automatic_min_savings_max_32b_5pct"
        ));
        assert!(plan.contains("late_materialization=true"));
        assert!(plan.contains("predicate_fields=title"));
        assert!(plan.contains("projection_fields=body,title"));
        assert!(plan.contains("column_fallback_reason=none"));
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert_eq!(metrics["column_batches"]["row_fetches_avoided"], 1);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_persist_hydrate_drop_column_batch_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE column_batch_metadata (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO column_batch_metadata (title, body) VALUES ('alpha', 'one')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_column_batch_metadata ON column_batch_metadata USING column (title, body) WITH (segment_size = 1)",
                    vec![],
                )
                .unwrap();
        }

        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.hydrate_catalog().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let metadata = cassie
            .midge
            .get_column_batch_metadata("column_batch_metadata", "idx_column_batch_metadata")
            .unwrap()
            .expect("column batch metadata should hydrate");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_metadata WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DROP INDEX idx_column_batch_metadata ON column_batch_metadata",
                vec![],
            )
            .unwrap();
        let dropped = cassie
            .midge
            .get_column_batch_metadata("column_batch_metadata", "idx_column_batch_metadata")
            .unwrap();

        // Assert
        assert_eq!(metadata.fields, vec!["title".to_string(), "body".to_string()]);
        assert_eq!(metadata.segments.len(), 1);
        assert_eq!(result.rows.len(), 1);
        assert!(dropped.is_none());
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_select_constant_codec_for_repeated_column_values() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_rle_codec");
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
                "CREATE TABLE column_batch_rle_codec (status TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for _ in 0..8 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO column_batch_rle_codec (status, body) VALUES ('active', 'same')",
                    vec![],
                )
                .unwrap();
        }

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_rle_codec ON column_batch_rle_codec USING column (status, body) WITH (segment_size = 8)",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .midge
            .get_column_batch_metadata("column_batch_rle_codec", "idx_column_batch_rle_codec")
            .unwrap()
            .expect("column batch metadata should exist");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT status, body FROM column_batch_rle_codec WHERE status = 'active'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(metadata.segments.len(), 1);
        let codecs = metadata.segments[0]
            .field_chunks
            .values()
            .collect::<Vec<_>>();
        assert_eq!(codecs.len(), 2);
        assert!(codecs.iter().all(|codec| codec.codec_name == "constant"));
        assert!(
            codecs
                .iter()
                .all(|codec| codec.encoded_len < codec.decoded_len)
        );
        assert!(codecs.iter().all(|codec| codec.value_count == 8));
        assert_eq!(result.rows.len(), 8);
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert!(metrics["column_batches"]["physical_bytes_total"]
            .as_u64()
            .unwrap()
            < metrics["column_batches"]["logical_bytes_total"]
                .as_u64()
                .unwrap());
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_to_row_blobs_for_corrupt_column_segment() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_corrupt_fallback");
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
                "CREATE TABLE column_batch_corrupt_fallback (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_corrupt_fallback (title, body) VALUES ('alpha', 'one')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_corrupt_fallback ON column_batch_corrupt_fallback USING column (title, body) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap();
        let segment_key = entries
            .into_iter()
            .find_map(|(key, value)| value.starts_with(b"CBC2").then_some(key))
            .expect("column batch segment should be persisted");
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(segment_key, b"not-json".to_vec(), None)
            .unwrap();
        tx.commit(WriteOptions::sync()).unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_corrupt_fallback WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::String("alpha".to_string()),
                Value::String("one".to_string()),
            ]]
        );
        assert_eq!(metrics["column_batches"]["decode_fallbacks"], 1);
        assert_eq!(metrics["column_batches"]["fallback_scans"], 1);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_prune_column_batch_segments_for_range_filter() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_range_pruning");
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
                "CREATE TABLE column_batch_range_pruning (label TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for score in 0..6 {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO column_batch_range_pruning (label, score) VALUES ('row{score}', {score})"
                    ),
                    vec![],
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_range_pruning ON column_batch_range_pruning USING column (label, score) WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT label FROM column_batch_range_pruning WHERE score >= 4 ORDER BY label",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT label FROM column_batch_range_pruning WHERE score >= 4 ORDER BY label",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("row4".to_string())],
                vec![Value::String("row5".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("column_batch_index=idx_column_batch_range_pruning"));
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert!(metrics["column_batches"]["skipped_segments"].as_u64().unwrap() > 0);
        assert!(metrics["column_batches"]["chunks_read"].as_u64().unwrap() < 8);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_sparse_nulls_during_column_batch_scan_pruning() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_sparse_null_pruning");
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
                "CREATE TABLE column_batch_sparse_null_pruning (title TEXT, category TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_sparse_null_pruning (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_sparse_null_pruning (title, category) VALUES ('beta', 'kept')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_sparse_null_pruning ON column_batch_sparse_null_pruning USING column (title, category) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, category FROM column_batch_sparse_null_pruning WHERE category IS NULL",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Null]]
        );
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert_eq!(metrics["column_batches"]["skipped_segments"], 1);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_column_batch_scan_metadata_after_update_delete() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_scan_rebuild");
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
                "CREATE TABLE column_batch_scan_rebuild (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_scan_rebuild (title, score) VALUES ('alpha', 1)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_scan_rebuild (title, score) VALUES ('beta', 2)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_scan_rebuild ON column_batch_scan_rebuild USING column (title, score) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE column_batch_scan_rebuild SET score = 10 WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM column_batch_scan_rebuild WHERE title = 'beta'",
                vec![],
            )
            .unwrap();
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM column_batch_scan_rebuild WHERE score >= 10",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .midge
            .get_column_batch_metadata("column_batch_scan_rebuild", "idx_column_batch_scan_rebuild")
            .unwrap()
            .expect("column batch metadata should exist");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(metadata.segments.len(), 1);
        assert_eq!(metadata.segments[0].summaries["score"].non_null_count, 1);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_to_row_blobs_for_active_session_changes() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_batch_session_fallback");
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
                "CREATE TABLE column_batch_session_fallback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_session_fallback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_session_fallback ON column_batch_session_fallback USING column (title) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_session_fallback (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM column_batch_session_fallback ORDER BY title",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );
        assert_eq!(metrics["column_batches"]["scans"], 0);
        assert_eq!(metrics["column_batches"]["row_blob_fetches"], 2);
        assert_eq!(
            metrics["column_batches"]["last_fallback_reason"],
            "session-changes"
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/derived_state_recovery.rs.
mod derived_state_recovery {
    use super::support_sql as support;
    use cassie::app::{Cassie, CassieSession};
    use cassie::midge::adapter::{
        set_column_batch_maintenance_failure_point, set_projection_hash_maintenance_failure_point,
        ColumnBatchScanDecision, RowFilter,
    };
    use cassie::types::{DataType, FieldSchema, Value};
    use support::{canonical_test_collection, canonical_test_index, data_dir, use_local_storage};

    use super::support_failpoints as failpoints;
    use failpoints::COLUMN_BATCH_FAILPOINT_GUARD;

    #[test]
    fn should_recover_column_batch_debt_without_serving_stale_rows() {
        // Arrange
        let _failpoint_guard = COLUMN_BATCH_FAILPOINT_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use_local_storage();
        let path = data_dir("derived_state_column_batch_recovery");
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
                "CREATE TABLE derived_state_docs (title TEXT, score INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO derived_state_docs (title, score) VALUES ('alpha', 1)",
                vec![],
            )
            .expect("insert row");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX derived_state_docs_column_idx ON derived_state_docs USING column (title, score)",
                vec![],
            )
            .expect("create column index");
        // Act
        set_column_batch_maintenance_failure_point(true);
        cassie
            .execute_sql(
                &session,
                "UPDATE derived_state_docs SET score = 2 WHERE title = 'alpha'",
                vec![],
            )
            .expect("durable write must succeed when maintenance fails");
        let collection = canonical_test_collection(&cassie, "derived_state_docs");
        let artifact_read = stale_artifact_read(&cassie, &collection);
        let fallback_result = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM derived_state_docs WHERE score = 2",
                vec![],
            )
            .expect("stale artifact must fall back to rows");
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("retry maintenance debt");
        let restarted_session = restarted.create_session("tester", None);
        let recovered_result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT title, score FROM derived_state_docs WHERE score = 2",
                vec![],
            )
            .expect("query after recovery");
        let collection = canonical_test_collection(&restarted, "derived_state_docs");
        let index = canonical_test_index(
            &restarted,
            &collection,
            "derived_state_docs_column_idx",
        );
        let metadata = restarted
            .midge
            .get_column_batch_metadata(&collection, &index)
            .expect("read metadata")
            .expect("metadata after recovery");
        // Assert
        assert_eq!(
            fallback_result.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(2)]]
        );
        assert!(matches!(
            artifact_read,
            ColumnBatchScanDecision::Fallback(reason)
                if reason.as_str() == "maintenance_pending"
        ));
        assert_eq!(recovered_result.rows, fallback_result.rows);
        assert_eq!(
            metadata.built_generation,
            restarted
                .midge
                .collection_generation(&collection)
                .expect("collection generation")
        );
    });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_recover_add_column_column_batch_debt_after_restart() {
        // Arrange
        let _failpoint_guard = COLUMN_BATCH_FAILPOINT_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use_local_storage();
        let path = data_dir("derived_state_add_column_batch_recovery");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            let collection = seed_add_column_batch_recovery(&cassie, &session);

            // Act
            set_column_batch_maintenance_failure_point(true);
            let alter_result = cassie.execute_sql(
                &session,
                "ALTER TABLE derived_state_add_column_batch_docs ADD COLUMN subtitle TEXT",
                vec![],
            );
            let schema_after_failure = cassie
                .midge
                .collection_schema(&collection)
                .expect("durable schema after interrupted maintenance");
            let debt_after_failure = cassie
                .midge
                .has_column_batch_maintenance_debt(&collection)
                .expect("read durable debt");
            let artifact_read = stale_artifact_read(&cassie, &collection);
            let fallback_result = cassie
                .execute_sql(
                    &session,
                    "SELECT title, score FROM derived_state_add_column_batch_docs WHERE score = 2",
                    vec![],
                )
                .expect("stale artifact must fall back to rows");
            let fallback_metrics = cassie.metrics();
            drop(cassie);
            let recovered = recover_add_column_batch(&path);

            // Assert
            assert!(alter_result.is_err());
            assert!(schema_after_failure
                .fields
                .iter()
                .any(|field| field.name == "subtitle"));
            assert!(debt_after_failure);
            assert!(matches!(
                artifact_read,
                ColumnBatchScanDecision::Fallback(reason)
                    if reason.as_str() == "maintenance_pending"
            ));
            assert_eq!(
                fallback_result.rows,
                vec![vec![Value::String("alpha".to_string()), Value::Int64(2)]]
            );
            assert!(fallback_metrics["column_batches"]["fallback_scans"]
                .as_u64()
                .is_some_and(|count| count > 0));
            assert_eq!(
                fallback_metrics["column_batches"]["last_fallback_reason"],
                "maintenance_pending"
            );
            assert_eq!(
                recovered.rows,
                vec![vec![
                    Value::String("alpha".to_string()),
                    Value::Int64(2),
                    Value::Null,
                ]]
            );
            assert!(matches!(
                recovered.artifact_read,
                ColumnBatchScanDecision::Hit(_)
            ));
            assert!(!recovered.debt);
            assert_eq!(recovered.built_generation, recovered.collection_generation);
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_recover_add_column_projection_hash_debt_after_restart() {
        // Arrange
        let _failpoint_guard = COLUMN_BATCH_FAILPOINT_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use_local_storage();
        let path = data_dir("derived_state_add_column_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let collection = "derived_state_add_column_docs";
        cassie
            .midge
            .create_collection(
                collection,
                cassie::types::Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .expect("create collection");
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("seed document");
        cassie
            .midge
            .rebuild_projection_hashes(collection)
            .expect("build initial hashes");

        // Act
        set_projection_hash_maintenance_failure_point(true);
        assert!(cassie
            .midge
            .alter_collection_add_column(
                collection,
                FieldSchema {
                    name: "subtitle".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            )
            .is_err());
        assert!(cassie
            .midge
            .has_projection_hash_maintenance_debt(collection)
            .expect("read durable debt"));
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("recover maintenance debt");

        // Assert
        assert!(!restarted
            .midge
            .has_projection_hash_maintenance_debt(collection)
            .expect("debt cleared"));
        assert!(restarted
            .midge
            .collection_schema(collection)
            .expect("schema")
            .fields
            .iter()
            .any(|field| field.name == "subtitle"));

        let _ = std::fs::remove_dir_all(path);
    }

    fn seed_add_column_batch_recovery(cassie: &Cassie, session: &CassieSession) -> String {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE derived_state_add_column_batch_docs (title TEXT, score INT)",
                vec![],
            )
            .expect("create table");
        cassie
        .execute_sql(
            session,
            "INSERT INTO derived_state_add_column_batch_docs (title, score) VALUES ('alpha', 2)",
            vec![],
        )
        .expect("insert row");
        cassie
        .execute_sql(
            session,
            "CREATE INDEX derived_state_add_column_batch_idx ON derived_state_add_column_batch_docs USING column (title, score)",
            vec![],
        )
        .expect("create column index");
        canonical_test_collection(cassie, "derived_state_add_column_batch_docs")
    }

    struct RecoveredColumnBatch {
        rows: Vec<Vec<Value>>,
        artifact_read: ColumnBatchScanDecision,
        debt: bool,
        built_generation: u64,
        collection_generation: u64,
    }

    fn recover_add_column_batch(path: &str) -> RecoveredColumnBatch {
        let restarted = Cassie::new_with_data_dir(path).expect("reopen Cassie");
        restarted.startup().expect("retry maintenance debt");
        let session = restarted.create_session("tester", None);
        let rows = restarted
        .execute_sql(
            &session,
            "SELECT title, score, subtitle FROM derived_state_add_column_batch_docs WHERE score = 2",
            vec![],
        )
        .expect("query after recovery")
        .rows;
        let collection =
            canonical_test_collection(&restarted, "derived_state_add_column_batch_docs");
        let index = canonical_test_index(
            &restarted,
            &collection,
            "derived_state_add_column_batch_idx",
        );
        let metadata = restarted
            .midge
            .get_column_batch_metadata(&collection, &index)
            .expect("read metadata")
            .expect("metadata after recovery");
        RecoveredColumnBatch {
            rows,
            artifact_read: stale_artifact_read(&restarted, &collection),
            debt: restarted
                .midge
                .has_column_batch_maintenance_debt(&collection)
                .expect("read recovered debt state"),
            built_generation: metadata.built_generation,
            collection_generation: restarted
                .midge
                .collection_generation(&collection)
                .expect("collection generation"),
        }
    }

    fn stale_artifact_read(cassie: &Cassie, collection: &str) -> ColumnBatchScanDecision {
        cassie
            .midge
            .scan_column_batch_projected_rows(
                collection,
                128,
                &["title".to_string(), "score".to_string()],
                Some(&RowFilter {
                    field: "score".to_string(),
                    value: serde_json::json!(2),
                }),
                None,
                None,
            )
            .expect("read stale artifact")
    }
}

// Formerly tests/projection_concurrency.rs.
mod projection_concurrency {
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    use cassie::app::{
        projection_concurrency_test_guard, set_projection_replay_prepare_barriers, Cassie,
        ProjectionReplayBatch, ProjectionReplayEvent,
    };
    use cassie::executor::{
        set_materialized_projection_replace_barriers,
        set_materialized_projection_replace_start_barriers,
    };
    use cassie::types::Value;
    use uuid::Uuid;

    fn data_dir(label: &str) -> String {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        std::env::temp_dir()
            .join(format!(
                "cassie-projection-concurrency-{label}-{}",
                Uuid::new_v4()
            ))
            .to_string_lossy()
            .into_owned()
    }

    fn replay_batch(
        projection: &str,
        batch_id: &str,
        event_id: &str,
        checkpoint: &str,
        position: u64,
    ) -> ProjectionReplayBatch {
        ProjectionReplayBatch {
            projection: projection.to_string(),
            source_identity: "projection-concurrency-source".to_string(),
            batch_id: batch_id.to_string(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: event_id.to_string(),
                checkpoint: checkpoint.to_string(),
                position: Some(position),
                document_id: event_id.to_string(),
                payload: Some(serde_json::json!({"marker": checkpoint})),
            }],
        }
    }

    fn canonical_projection(cassie: &Cassie, collection: &str) -> String {
        cassie
            .catalog
            .get_schema(collection)
            .map_or_else(|| collection.to_string(), |schema| schema.collection)
    }

    #[test]
    fn should_serialize_concurrent_materialized_projection_refreshes() {
        // Arrange
        let _guard = projection_concurrency_test_guard();
        let path = data_dir("refresh");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
        cassie.startup().expect("startup");
        let setup = cassie.create_session("setup", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE projection_concurrent_source (title TEXT, score INT)",
                vec![],
            )
            .expect("create source");
        cassie
            .execute_sql(
                &setup,
                "INSERT INTO projection_concurrent_source VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .expect("seed source");
        cassie
        .execute_sql(
            &setup,
            "CREATE MATERIALIZED PROJECTION projection_concurrent AS SELECT title FROM projection_concurrent_source WHERE score > 1",
            vec![],
        )
        .expect("create projection");
        let dropped = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let replace_ready = Arc::new(Barrier::new(2));
        let replace_resume = Arc::new(Barrier::new(2));
        set_materialized_projection_replace_start_barriers(
            Some(Arc::clone(&replace_ready)),
            Some(Arc::clone(&replace_resume)),
        );
        set_materialized_projection_replace_barriers(
            Some(Arc::clone(&dropped)),
            Some(Arc::clone(&resume)),
        );

        // Act
        let first_cassie = Arc::clone(&cassie);
        let first = std::thread::spawn(move || {
            let session = first_cassie.create_session("first", None);
            first_cassie.execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_concurrent",
                vec![],
            )
        });
        replace_ready.wait();
        cassie
            .execute_sql(
                &setup,
                "INSERT INTO projection_concurrent_source VALUES ('charlie', 3)",
                vec![],
            )
            .expect("mutate source between refresh snapshots");
        replace_resume.wait();
        dropped.wait();
        let (second_tx, second_rx) = mpsc::channel();
        let second_cassie = Arc::clone(&cassie);
        let second = std::thread::spawn(move || {
            let session = second_cassie.create_session("second", None);
            let result = second_cassie.execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_concurrent",
                vec![],
            );
            second_tx.send(result).expect("send second refresh result");
        });
        let early_second = second_rx.recv_timeout(Duration::from_millis(250)).ok();
        resume.wait();
        let first_result = first.join().expect("first refresh thread");
        second.join().expect("second refresh thread");
        let second_result = early_second.unwrap_or_else(|| {
            second_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("second refresh should complete")
        });
        let rows = cassie
            .execute_sql(
                &setup,
                "SELECT title FROM projection_concurrent ORDER BY title",
                vec![],
            )
            .expect("query refreshed projection");

        // Assert
        first_result.expect("first refresh should succeed");
        second_result.expect("second refresh should succeed");
        assert_eq!(
            rows.rows,
            vec![
                vec![Value::String("bravo".to_string())],
                vec![Value::String("charlie".to_string())],
            ]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_concurrent_replay_checkpoint_monotonic() {
        // Arrange
        let _guard = projection_concurrency_test_guard();
        let path = data_dir("replay-checkpoint");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
        cassie.startup().expect("startup");
        let setup = cassie.create_session("setup", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE projection_checkpoint_target (marker TEXT)",
                vec![],
            )
            .expect("create replay target");
        let projection = canonical_projection(&cassie, "projection_checkpoint_target");
        let ready = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        set_projection_replay_prepare_barriers(
            Some("batch-low".to_string()),
            Some(Arc::clone(&ready)),
            Some(Arc::clone(&resume)),
        );

        // Act
        let low_cassie = Arc::clone(&cassie);
        let low_projection = projection.clone();
        let low = std::thread::spawn(move || {
            low_cassie.replay_projection_batch(replay_batch(
                &low_projection,
                "batch-low",
                "event-low",
                "checkpoint-5",
                5,
            ))
        });
        ready.wait();
        let (high_tx, high_rx) = mpsc::channel();
        let high_cassie = Arc::clone(&cassie);
        let high_projection = projection.clone();
        let high = std::thread::spawn(move || {
            let result = high_cassie.replay_projection_batch(replay_batch(
                &high_projection,
                "batch-high",
                "event-high",
                "checkpoint-10",
                10,
            ));
            high_tx.send(result).expect("send high replay result");
        });
        let early_high = high_rx.recv_timeout(Duration::from_millis(250)).ok();
        resume.wait();
        low.join().expect("low replay thread").expect("low replay");
        high.join().expect("high replay thread");
        early_high
            .unwrap_or_else(|| {
                high_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("high replay should complete")
            })
            .expect("high replay");
        let metadata = cassie
            .catalog
            .get_projection_metadata(&projection)
            .expect("projection metadata");

        // Assert
        assert_eq!(metadata.source_position, Some(10));
        assert_eq!(metadata.source_checkpoint.as_deref(), Some("checkpoint-10"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_apply_concurrent_duplicate_replay_event_once() {
        // Arrange
        let _guard = projection_concurrency_test_guard();
        let path = data_dir("replay-duplicate");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
        cassie.startup().expect("startup");
        let setup = cassie.create_session("setup", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE projection_duplicate_target (marker TEXT)",
                vec![],
            )
            .expect("create replay target");
        let projection = canonical_projection(&cassie, "projection_duplicate_target");
        let ready = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        set_projection_replay_prepare_barriers(
            Some("batch-paused".to_string()),
            Some(Arc::clone(&ready)),
            Some(Arc::clone(&resume)),
        );

        // Act
        let paused_cassie = Arc::clone(&cassie);
        let paused_projection = projection.clone();
        let paused = std::thread::spawn(move || {
            paused_cassie.replay_projection_batch(replay_batch(
                &paused_projection,
                "batch-paused",
                "event-shared",
                "checkpoint-1",
                1,
            ))
        });
        ready.wait();
        let (racing_tx, racing_rx) = mpsc::channel();
        let racing_cassie = Arc::clone(&cassie);
        let racing_projection = projection.clone();
        let racing = std::thread::spawn(move || {
            let result = racing_cassie.replay_projection_batch(replay_batch(
                &racing_projection,
                "batch-racing",
                "event-shared",
                "checkpoint-1",
                1,
            ));
            racing_tx.send(result).expect("send racing replay result");
        });
        let early_racing = racing_rx.recv_timeout(Duration::from_millis(250)).ok();
        resume.wait();
        let paused = paused
            .join()
            .expect("paused replay thread")
            .expect("paused replay");
        racing.join().expect("racing replay thread");
        let racing = early_racing
            .unwrap_or_else(|| {
                racing_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("racing replay should complete")
            })
            .expect("racing replay");

        // Assert
        assert_eq!(paused.applied_event_count + racing.applied_event_count, 1);
        assert_eq!(
            paused.skipped_duplicate_count + racing.skipped_duplicate_count,
            1
        );
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/projection_consistency.rs.
mod projection_consistency {
    use cassie::app::{Cassie, CassieError, ProjectionManifestExportOptions};
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::Value;
    use uuid::Uuid;

    use super::support_sql as support;
    use support::*;

    fn canonical_collection(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    fn create_manifest_source(
        label: &str,
        title: &str,
    ) -> (Cassie, String, String, ProjectionManifestExportOptions) {
        use_local_storage();
        let dir = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&dir).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE consistency_docs (title TEXT, body TEXT, embedding VECTOR(2))",
                vec![],
            )
            .expect("create table");
        let projection = canonical_collection("consistency_docs");
        cassie
            .midge
            .put_document(
                &projection,
                Some("doc-2".to_string()),
                serde_json::json!({
                    "title": title,
                    "body": "sensitive body text",
                    "embedding": [0.25, 0.75]
                }),
            )
            .expect("insert doc-2");
        cassie
            .midge
            .put_document(
                &projection,
                Some("doc-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "body": "secret password bind value",
                    "embedding": [1.0, 0.0]
                }),
            )
            .expect("insert doc-1");

        let mut options = ProjectionManifestExportOptions::for_instance(label);
        options.generated_ms = Some(4_000_000_000_000);
        options.ttl_ms = Some(86_400_000);
        options.include_row_hashes = true;
        (cassie, dir, projection, options)
    }

    async fn spawn_rest_server(
        cassie: Cassie,
    ) -> (String, tokio::task::JoinHandle<Result<(), CassieError>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        (format!("http://{addr}"), server)
    }

    async fn login_cookie(client: &reqwest::Client, base_url: &str) -> String {
        client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "root",
                "password": "topsecret"
            }))
            .send()
            .await
            .expect("login request")
            .headers()
            .get("set-cookie")
            .expect("session cookie")
            .to_str()
            .expect("session cookie value")
            .split(';')
            .next()
            .expect("session cookie pair")
            .to_string()
    }

    #[test]
    fn should_export_projection_verification_manifest_with_canonical_ordering() {
        // Arrange
        let (cassie, _dir, projection, options) = create_manifest_source("instance-a", "bravo");

        // Act
        let first = cassie
            .export_projection_verification_manifest(&projection, options.clone())
            .expect("first manifest");
        let second = cassie
            .export_projection_verification_manifest(&projection, options)
            .expect("second manifest");

        // Assert
        assert_eq!(first.manifest_version, 1);
        assert_eq!(first.instance_id, "instance-a");
        assert_eq!(first.projection_id, projection);
        assert_eq!(first.generated_ms, 4_000_000_000_000);
        assert_eq!(first.manifest_digest, second.manifest_digest);
        assert!(first
            .ranges
            .windows(2)
            .all(|pair| pair[0].range_id <= pair[1].range_id));
        assert!(first
            .row_hashes
            .windows(2)
            .all(|pair| pair[0].row_id <= pair[1].row_id));
    }

    #[test]
    fn should_compare_equal_manifests_as_consistent() {
        // Arrange
        let (left, _left_dir, projection, left_options) =
            create_manifest_source("instance-a", "bravo");
        let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "bravo");
        let left_manifest = left
            .export_projection_verification_manifest(&projection, left_options)
            .expect("left manifest");
        let right_manifest = right
            .export_projection_verification_manifest(&projection, right_options)
            .expect("right manifest");

        // Act
        let report = left
            .compare_projection_verification_manifests(vec![right_manifest, left_manifest])
            .expect("compare manifests");

        // Assert
        assert_eq!(report.state, "consistent");
        assert_eq!(report.manifest_count, 2);
        assert_eq!(
            report.instance_ids,
            vec!["instance-a".to_string(), "instance-b".to_string()]
        );
        assert_eq!(report.mismatch_count, 0);
    }

    #[test]
    fn should_report_row_level_divergence_when_hashes_are_available() {
        // Arrange
        let (left, _left_dir, projection, left_options) =
            create_manifest_source("instance-a", "bravo");
        let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "charlie");
        let left_manifest = left
            .export_projection_verification_manifest(&projection, left_options)
            .expect("left manifest");
        let right_manifest = right
            .export_projection_verification_manifest(&projection, right_options)
            .expect("right manifest");

        // Act
        let report = left
            .compare_projection_verification_manifests(vec![left_manifest, right_manifest])
            .expect("compare manifests");

        // Assert
        assert_eq!(report.state, "divergent");
        assert_eq!(report.mismatch_count, 1);
        assert_eq!(report.divergent_range_count, 1);
        assert_eq!(report.divergent_row_count, 1);
        assert!(report.diagnostic_sample.contains(&"row:doc-2".to_string()));
    }

    #[test]
    fn should_report_stale_manifest_state() {
        // Arrange
        let (left, _left_dir, projection, left_options) =
            create_manifest_source("instance-a", "bravo");
        let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "bravo");
        let left_manifest = left
            .export_projection_verification_manifest(&projection, left_options)
            .expect("left manifest");
        let mut stale_manifest = right
            .export_projection_verification_manifest(&projection, right_options.clone())
            .expect("stale manifest");
        stale_manifest.expires_at_ms = 1;

        // Act
        let stale = left
            .compare_projection_verification_manifests(vec![left_manifest.clone(), stale_manifest])
            .expect("compare stale manifests");

        // Assert
        assert_eq!(stale.state, "stale");
        assert_eq!(stale.stale_manifest_count, 1);
    }

    #[test]
    fn should_reject_incompatible_hash_metadata() {
        // Arrange
        let (left, _left_dir, projection, left_options) =
            create_manifest_source("instance-a", "bravo");
        let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "bravo");
        let left_manifest = left
            .export_projection_verification_manifest(&projection, left_options)
            .expect("left manifest");
        let mut incompatible_manifest = right
            .export_projection_verification_manifest(&projection, right_options)
            .expect("incompatible manifest");
        incompatible_manifest.hash.algorithm = "other-hash".to_string();
        incompatible_manifest.manifest_digest = String::new();

        // Act
        let incompatible = left
            .compare_projection_verification_manifests(vec![left_manifest, incompatible_manifest])
            .expect("compare incompatible manifests");

        // Assert
        assert_eq!(incompatible.state, "incompatible");
        assert_eq!(incompatible.incompatible_manifest_count, 1);
        assert!(incompatible
            .diagnostic_sample
            .contains(&"hash-algorithm".to_string()));
    }

    #[test]
    fn should_exclude_sensitive_values_from_manifest() {
        // Arrange
        let (cassie, _dir, projection, mut options) = create_manifest_source("instance-a", "bravo");
        options.include_row_hashes = true;

        // Act
        let manifest = cassie
            .export_projection_verification_manifest(&projection, options)
            .expect("manifest");
        let serialized = serde_json::to_string(&manifest).expect("serialize manifest");

        // Assert
        assert!(!serialized.contains("sensitive body text"));
        assert!(!serialized.contains("secret password bind value"));
        assert!(!serialized.contains("bravo"));
        assert!(!serialized.contains("0.25"));
        assert!(!serialized.contains("0.75"));
    }

    #[test]
    fn should_rehydrate_persisted_consistency_report_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_consistency_restart");
        let report_id = {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE consistency_restart_docs (title TEXT)",
                    vec![],
                )
                .expect("create table");
            let projection = canonical_collection("consistency_restart_docs");
            cassie
                .midge
                .put_document(
                    &projection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .expect("insert doc");
            let mut left_options = ProjectionManifestExportOptions::for_instance("instance-a");
            left_options.generated_ms = Some(4_000_000_000_000);
            let mut right_options = ProjectionManifestExportOptions::for_instance("instance-b");
            right_options.generated_ms = Some(4_000_000_000_000);
            let left_manifest = cassie
                .export_projection_verification_manifest(&projection, left_options)
                .expect("left manifest");
            let right_manifest = cassie
                .export_projection_verification_manifest(&projection, right_options)
                .expect("right manifest");

            // Act
            let report = cassie
                .compare_projection_verification_manifests(vec![left_manifest, right_manifest])
                .expect("compare manifests");
            cassie.shutdown();
            report.report_id
        };

        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restart startup");
        let session = restarted.create_session("tester", None);
        let reports = restarted
        .execute_sql(
            &session,
            &format!(
                "SELECT state, manifest_count FROM pg_catalog.pg_projection_consistency_reports WHERE report_id = '{report_id}'"
            ),
            vec![],
        )
        .expect("query report");

        // Assert
        assert_eq!(
            reports.rows,
            vec![vec![
                Value::String("consistent".to_string()),
                Value::Int64(2)
            ]]
        );

        restarted.shutdown();
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_record_consistency_metrics() {
        // Arrange
        let (left, _left_dir, projection, left_options) =
            create_manifest_source("instance-a", "bravo");
        let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "charlie");
        let before = left.metrics();
        let left_manifest = left
            .export_projection_verification_manifest(&projection, left_options)
            .expect("left manifest");
        let right_manifest = right
            .export_projection_verification_manifest(&projection, right_options)
            .expect("right manifest");

        // Act
        let _ = left
            .compare_projection_verification_manifests(vec![left_manifest, right_manifest])
            .expect("compare manifests");
        let after = left.metrics();

        // Assert
        assert_eq!(
            after["projections"]["consistency_exports"].as_u64(),
            before["projections"]["consistency_exports"]
                .as_u64()
                .map(|value| value + 1)
        );
        assert_eq!(
            after["projections"]["consistency_checks"].as_u64(),
            before["projections"]["consistency_checks"]
                .as_u64()
                .map(|value| value + 1)
        );
        assert!(
            after["projections"]["consistency_mismatches"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
    }

    #[test]
    fn should_support_admin_rest_manifest_consistency_workflow() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_consistency_rest");
        let config = CassieRuntimeConfig {
            password: "topsecret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE consistency_rest_docs (title TEXT)",
                    vec![],
                )
                .expect("create table");
            let projection = canonical_collection("consistency_rest_docs");
            cassie
                .midge
                .put_document(
                    &projection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .expect("insert doc");

            let (base_url, server) = spawn_rest_server(cassie.clone()).await;
            let client = reqwest::Client::new();
            let admin_cookie = login_cookie(&client, &base_url).await;
            let nonce = Uuid::new_v4().to_string();

            // Act
            let unauthorized = client
                .post(format!(
                "{base_url}/api/v1/admin/projections/consistency_rest_docs/verification-manifest"
            ))
                .json(&serde_json::json!({"instance_id": format!("unauthorized-{nonce}")}))
                .send()
                .await
                .expect("unauthorized request");
            let manifest = client
                .post(format!(
                "{base_url}/api/v1/admin/projections/consistency_rest_docs/verification-manifest"
            ))
                .header("cookie", &admin_cookie)
                .json(&serde_json::json!({
                    "instance_id": "rest-a",
                    "generated_ms": 4_000_000_000_000_u64,
                    "ttl_ms": 86_400_000_u64,
                    "include_row_hashes": true
                }))
                .send()
                .await
                .expect("manifest request")
                .json::<serde_json::Value>()
                .await
                .expect("manifest json");
            let report = client
                .post(format!(
                    "{base_url}/api/v1/admin/projection-consistency-checks"
                ))
                .header("cookie", &admin_cookie)
                .json(&serde_json::json!({"manifests": [manifest.clone(), manifest]}))
                .send()
                .await
                .expect("compare request")
                .json::<serde_json::Value>()
                .await
                .expect("report json");

            // Assert
            assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
            assert_eq!(report["state"], "consistent");
            assert_eq!(report["manifest_count"], 2);

            server.abort();
            let _ = server.await;
        });

        cassie.shutdown();
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_support_restful_projection_consistency_aliases() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_consistency_restful_aliases");
        let config = CassieRuntimeConfig {
            password: "topsecret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE consistency_rest_alias_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        let projection = canonical_collection("consistency_rest_alias_docs");
        cassie
            .midge
            .put_document(
                &projection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("insert doc");

        let (base_url, server) = spawn_rest_server(cassie.clone()).await;
        let client = reqwest::Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let manifest_response = client
            .post(format!(
                "{base_url}/api/v1/admin/projections/consistency_rest_alias_docs/verification-manifests"
            ))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "instance_id": "rest-a",
                "generated_ms": 4_000_000_000_000_u64,
                "ttl_ms": 86_400_000_u64,
                "include_row_hashes": true
            }))
            .send()
            .await
            .expect("manifest request");
        let manifest_status = manifest_response.status();
        let manifest = manifest_response
            .json::<serde_json::Value>()
            .await
            .expect("manifest json");

        let report_response = client
            .post(format!("{base_url}/api/v1/admin/projection-consistency-reports"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({"manifests": [manifest.clone(), manifest]}))
            .send()
            .await
            .expect("report request");
        let report_status = report_response.status();
        let report = report_response
            .json::<serde_json::Value>()
            .await
            .expect("report json");

        let report_list_response = client
            .get(format!("{base_url}/api/v1/admin/projection-consistency-reports"))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("reports request");
        let report_list_status = report_list_response.status();
        let reports = report_list_response
            .json::<serde_json::Value>()
            .await
            .expect("reports json");

        // Assert
        assert_eq!(manifest_status, reqwest::StatusCode::OK);
        assert_eq!(report_status, reqwest::StatusCode::OK);
        assert_eq!(report_list_status, reqwest::StatusCode::OK);
        assert_eq!(report["state"], "consistent");
        assert_eq!(report["manifest_count"], 2);
        assert!(
            reports["reports"]
                .as_array()
                .expect("reports")
                .iter()
                .any(|entry| entry["report_id"] == report["report_id"]),
            "expected created report to appear in GET /projection-consistency-reports"
        );

        server.abort();
        let _ = server.await;
    });

        cassie.shutdown();
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/projection_diffing.rs.
mod projection_diffing {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::sql::ast::QueryStatement;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_parse_projection_diff_command() {
        // Arrange
        let diff_sql =
            "DIFF PROJECTION left_docs VERSION v1 WITH right_docs VERSION v2 LIMIT 10 AFTER row-1";

        // Act
        let diff = cassie::sql::parse_statement(diff_sql).unwrap();

        // Assert
        let QueryStatement::DiffProjection(diff) = diff.statement else {
            panic!("expected DIFF PROJECTION");
        };
        assert_eq!(diff.left.name, "left_docs");
        assert_eq!(diff.left.version_id.as_deref(), Some("v1"));
        assert_eq!(diff.right.name, "right_docs");
        assert_eq!(diff.right.version_id.as_deref(), Some("v2"));
        assert_eq!(diff.limit, Some(10));
        assert_eq!(diff.after.as_deref(), Some("row-1"));
    }

    #[test]
    fn should_parse_projection_compare_command() {
        // Arrange
        let compare_sql = "COMPARE PROJECTION left_docs WITH MANIFEST '{\"root_digest\":\"abc\"}'";

        // Act
        let compare = cassie::sql::parse_statement(compare_sql).unwrap();

        // Assert
        let QueryStatement::CompareProjection(compare) = compare.statement else {
            panic!("expected COMPARE PROJECTION");
        };
        assert_eq!(compare.target.name, "left_docs");
        assert!(compare.manifest.contains("root_digest"));
    }

    #[test]
    fn should_diff_projection_hashes_deterministically() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_diff_hashes");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, "CREATE TABLE diff_left (title TEXT)", vec![])
                .unwrap();
            cassie
                .execute_sql(&session, "CREATE TABLE diff_right (title TEXT)", vec![])
                .unwrap();
            let left = canonical_test_collection(&cassie, "diff_left");
            let right = canonical_test_collection(&cassie, "diff_right");
            cassie
                .midge
                .put_document(
                    &left,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    &right,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "bravo"}),
                )
                .unwrap();

            // Act
            let diff = cassie
                .execute_sql(
                    &session,
                    "DIFF PROJECTION diff_left WITH diff_right LIMIT 5",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(diff.columns[0].name, "row_id");
            assert_eq!(diff.rows[0][0], Value::String("doc-1".to_string()));
            assert_eq!(diff.rows[0][1], Value::String("changed".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_compare_projection_manifest_root_digest() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_compare_manifest");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE compare_docs (title TEXT)", vec![])
            .unwrap();
        let collection = canonical_test_collection(&cassie, "compare_docs");
        cassie
            .midge
            .put_document(
                &collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        let root = cassie
            .midge
            .root_hash(&collection)
            .unwrap()
            .expect("root hash");
        let sql = format!(
            "COMPARE PROJECTION compare_docs WITH MANIFEST '{{\"root_digest\":\"{}\",\"algorithm\":\"{}\",\"digest_length\":{},\"canonical_encoder_version\":{},\"row_hash_version\":{},\"range_hash_version\":{},\"root_hash_version\":{}}}'",
            root.digest,
            root.algorithm,
            root.digest_length,
            root.canonical_encoder_version,
            root.row_hash_version,
            root.range_hash_version,
            root.root_hash_version
        );

        // Act
        let compared = cassie.execute_sql(&session, &sql, vec![]).unwrap();

        // Assert
        assert_eq!(compared.rows[0][1], Value::String("equal".to_string()));
        assert_eq!(compared.rows[0][4], Value::Int64(0));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_report_projection_diff_resume_cursor_for_bounded_output() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_diff_resume_cursor");
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
                    "CREATE TABLE diff_cursor_left (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE diff_cursor_right (title TEXT)",
                    vec![],
                )
                .unwrap();
            let left = canonical_test_collection(&cassie, "diff_cursor_left");
            let right = canonical_test_collection(&cassie, "diff_cursor_right");
            for row_id in ["doc-1", "doc-2", "doc-3"] {
                cassie
                    .midge
                    .put_document(
                        &left,
                        Some(row_id.to_string()),
                        serde_json::json!({"title": "left"}),
                    )
                    .unwrap();
                cassie
                    .midge
                    .put_document(
                        &right,
                        Some(row_id.to_string()),
                        serde_json::json!({"title": "right"}),
                    )
                    .unwrap();
            }

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "DIFF PROJECTION diff_cursor_left WITH diff_cursor_right LIMIT 1",
                    vec![],
                )
                .unwrap();
            let cursor = match &first.rows[0][5] {
                Value::String(value) => value.clone(),
                other => panic!("expected resume cursor, got {other:?}"),
            };
            let second = cassie
                .execute_sql(
                    &session,
                    &format!(
                    "DIFF PROJECTION diff_cursor_left WITH diff_cursor_right LIMIT 5 AFTER {cursor}"
                ),
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(first.rows[0][0], Value::String("doc-1".to_string()));
            assert_eq!(first.rows[0][6], Value::Bool(false));
            assert_eq!(second.rows[0][0], Value::String("doc-2".to_string()));
            assert_eq!(second.rows[0][6], Value::Bool(true));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_projection_manifest_missing_hash_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_compare_missing_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE compare_missing_docs (title TEXT)", vec![])
            .unwrap();
        let collection = canonical_test_collection(&cassie, "compare_missing_docs");
        cassie
            .midge
            .put_document(
                &collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        let root = cassie
            .midge
            .root_hash(&collection)
            .unwrap()
            .expect("root hash");
        let sql = format!(
            "COMPARE PROJECTION compare_missing_docs WITH MANIFEST '{{\"root_digest\":\"{}\"}}'",
            root.digest
        );

        // Act
        let compared = cassie.execute_sql(&session, &sql, vec![]).unwrap();
        let reports = cassie
            .execute_sql(
                &session,
                "SELECT state, compatibility_status FROM pg_catalog.pg_projection_comparison_reports",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(compared.rows[0][1], Value::String("unverifiable".to_string()));
        assert_eq!(
            reports.rows[0],
            vec![
                Value::String("unverifiable".to_string()),
                Value::String("incompatible-missing-algorithm".to_string()),
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_persist_projection_comparison_report_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_comparison_report_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let report_id = {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE comparison_report_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            let collection = canonical_test_collection(&cassie, "comparison_report_docs");
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            let root = cassie
                .midge
                .root_hash(&collection)
                .unwrap()
                .expect("root hash");
            let sql = format!(
                "COMPARE PROJECTION comparison_report_docs WITH MANIFEST '{{\"root_digest\":\"{}\",\"algorithm\":\"{}\",\"digest_length\":{},\"canonical_encoder_version\":{},\"row_hash_version\":{},\"range_hash_version\":{},\"root_hash_version\":{}}}'",
                root.digest,
                root.algorithm,
                root.digest_length,
                root.canonical_encoder_version,
                root.row_hash_version,
                root.range_hash_version,
                root.root_hash_version
            );

            // Act
            let compared = cassie.execute_sql(&session, &sql, vec![]).unwrap();
            let report_id = match &compared.rows[0][5] {
                Value::String(value) => value.clone(),
                other => panic!("expected report id, got {other:?}"),
            };
            cassie.shutdown();
            report_id
        };

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let reports = restarted
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, compatibility_status, mismatch_count FROM pg_catalog.pg_projection_comparison_reports WHERE report_id = '{report_id}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            reports.rows,
            vec![vec![
                Value::String("equal".to_string()),
                Value::String("compatible".to_string()),
                Value::Int64(0),
            ]]
        );

        restarted.shutdown();
        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/projection_hash_recovery.rs.
mod projection_hash_recovery {
    use cassie::app::Cassie;
    use cassie::midge::adapter::set_projection_hash_maintenance_failure_point;

    use super::support_sql as support;
    use support::{canonical_test_collection, data_dir, use_local_storage};

    #[test]
    fn should_retry_projection_hash_debt_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_hash_debt_recovery");
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
                    "CREATE TABLE projection_hash_debt_docs (title TEXT)",
                    vec![],
                )
                .expect("create table");
            let collection = canonical_test_collection(&cassie, "projection_hash_debt_docs");
            cassie
                .midge
                .put_fresh_documents(
                    &collection,
                    vec![(
                        Some("doc-1".to_string()),
                        serde_json::json!({"title": "before"}),
                    )],
                )
                .expect("seed current projection hashes");
            assert!(cassie
                .midge
                .root_hash(&collection)
                .expect("read current root")
                .is_some());

            // Act
            set_projection_hash_maintenance_failure_point(true);
            cassie
                .midge
                .put_fresh_documents(
                    &collection,
                    vec![(
                        Some("doc-2".to_string()),
                        serde_json::json!({"title": "alpha"}),
                    )],
                )
                .expect("durable write must not return a maintenance failure");
            assert!(cassie
                .midge
                .has_projection_hash_maintenance_debt(&collection)
                .expect("read durable debt"));
            assert!(cassie
                .midge
                .root_hash(&collection)
                .expect("read stale root")
                .is_none());
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
            restarted.startup().expect("recover projection hashes");

            // Assert
            assert!(!restarted
                .midge
                .has_projection_hash_maintenance_debt(&collection)
                .expect("debt should be cleared after recovery"));
            assert!(restarted
                .midge
                .root_hash(&collection)
                .expect("read recovered root")
                .is_some());
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/projection_lifecycle.rs.
mod projection_lifecycle {
    #![allow(unused_imports, dead_code)]

    use cassie::app::{Cassie, CassieSession};
    use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
    use cassie::catalog::ProjectionVerificationState;
    use cassie::sql::ast::{
        AlterMaterializedProjectionOperation, CopyFormat, CopyStatement, QueryStatement,
    };
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    fn execute_statement(cassie: &Cassie, session: &CassieSession, sql: &str) {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }

    fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        cassie.execute_sql(session, sql, vec![]).unwrap().rows
    }

    fn seed_materialized_projection_version_fixture(
        cassie: &Cassie,
        session: &CassieSession,
    ) -> String {
        execute_statement(
            cassie,
            session,
            "CREATE TABLE projection_version_docs (title TEXT)",
        );
        execute_statement(
            cassie,
            session,
            "INSERT INTO projection_version_docs (title) VALUES ('alpha')",
        );
        execute_statement(
        cassie,
        session,
        "CREATE MATERIALIZED PROJECTION projection_versioned AS SELECT title FROM projection_version_docs",
    );
        let projection = cassie
            .catalog
            .get_materialized_projection("projection_versioned")
            .expect("materialized projection metadata")
            .collection;
        execute_statement(
            cassie,
            session,
            "INSERT INTO projection_version_docs (title) VALUES ('bravo')",
        );
        projection
    }

    fn projection_version_rows(
        cassie: &Cassie,
        session: &CassieSession,
        projection: &str,
    ) -> Vec<Vec<Value>> {
        query_rows(
        cassie,
        session,
        &format!(
            "SELECT version_id, state FROM pg_catalog.pg_projection_versions WHERE projection_name = '{projection}' ORDER BY version_id"
        ),
    )
    }

    #[test]
    fn should_parse_materialized_projection_lifecycle_commands() {
        // Arrange
        let create_sql =
            "CREATE MATERIALIZED PROJECTION projection_ready AS SELECT title FROM docs";
        let activate_sql =
            "ALTER MATERIALIZED PROJECTION projection_ready ACTIVATE VERSION v2 UNSAFE";
        let drop_version_sql = "DROP MATERIALIZED PROJECTION VERSION projection_ready VERSION v1";

        // Act
        let create = cassie::sql::parse_statement(create_sql).unwrap();
        let activate = cassie::sql::parse_statement(activate_sql).unwrap();
        let drop_version = cassie::sql::parse_statement(drop_version_sql).unwrap();

        // Assert
        let QueryStatement::CreateMaterializedProjection(create) = create.statement else {
            panic!("expected CREATE MATERIALIZED PROJECTION");
        };
        assert_eq!(create.name, "projection_ready");
        assert_eq!(create.query, "SELECT title FROM docs");

        let QueryStatement::AlterMaterializedProjection(activate) = activate.statement else {
            panic!("expected ALTER MATERIALIZED PROJECTION");
        };
        match activate.operation {
            AlterMaterializedProjectionOperation::ActivateVersion {
                version_id,
                unsafe_override,
            } => {
                assert_eq!(version_id, "v2");
                assert!(unsafe_override);
            }
            AlterMaterializedProjectionOperation::BuildVersion => {
                panic!("expected activate version")
            }
        }

        let QueryStatement::DropMaterializedProjectionVersion(drop_version) =
            drop_version.statement
        else {
            panic!("expected DROP MATERIALIZED PROJECTION VERSION");
        };
        assert_eq!(drop_version.name, "projection_ready");
        assert_eq!(drop_version.version_id, "v1");
    }

    #[test]
    fn should_replay_projection_batch_idempotently_with_checkpoint_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_replay_idempotent");
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
                "CREATE TABLE projection_replay_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_docs");
        let batch = ProjectionReplayBatch {
            projection: projection.clone(),
            source_identity: "orders-stream".to_string(),
            batch_id: "batch-1".to_string(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "event-1".to_string(),
                checkpoint: "checkpoint-1".to_string(),
                position: Some(1),
                document_id: "doc-1".to_string(),
                payload: Some(serde_json::json!({"title": "alpha"})),
            }],
        };

        // Act
        let first = cassie.replay_projection_batch(batch.clone()).unwrap();
        let duplicate = cassie.replay_projection_batch(batch).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT source_identity, source_checkpoint, last_applied_event_id, replay_batch_id, freshness FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(first.applied_event_count, 1);
        assert_eq!(duplicate.skipped_duplicate_count, 1);
        assert_eq!(selected.rows, vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(
            checkpoint.rows,
            vec![vec![
                Value::String("orders-stream".to_string()),
                Value::String("checkpoint-1".to_string()),
                Value::String("event-1".to_string()),
                Value::String("batch-1".to_string()),
                Value::String("fresh".to_string()),
            ]]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["projections"]["replay_batches"].as_u64(), Some(2));
        assert_eq!(
            metrics["projections"]["replay_duplicates_skipped"].as_u64(),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_skip_existing_projection_events_while_applying_new_replay_events() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_replay_mixed_duplicate_batch");
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
                    "CREATE TABLE projection_replay_mixed_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            let projection = canonical_test_collection(&cassie, "projection_replay_mixed_docs");
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
                        payload: Some(serde_json::json!({"title": "alpha"})),
                    }],
                })
                .unwrap();

            // Act
            let replay = cassie
                .replay_projection_batch(ProjectionReplayBatch {
                    projection,
                    source_identity: "orders-stream".to_string(),
                    batch_id: "batch-2".to_string(),
                    lag: 0,
                    events: vec![
                        ProjectionReplayEvent {
                            event_id: "event-1".to_string(),
                            checkpoint: "checkpoint-1".to_string(),
                            position: Some(1),
                            document_id: "doc-1".to_string(),
                            payload: Some(serde_json::json!({"title": "alpha-updated"})),
                        },
                        ProjectionReplayEvent {
                            event_id: "event-2".to_string(),
                            checkpoint: "checkpoint-2".to_string(),
                            position: Some(2),
                            document_id: "doc-2".to_string(),
                            payload: Some(serde_json::json!({"title": "bravo"})),
                        },
                    ],
                })
                .unwrap();
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM projection_replay_mixed_docs ORDER BY title",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(replay.applied_event_count, 1);
            assert_eq!(replay.skipped_duplicate_count, 1);
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("alpha".to_string())],
                    vec![Value::String("bravo".to_string())],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_mark_large_replay_hashes_stale_without_eager_rebuild() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_replay_large_hashes_stale");
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
                    "CREATE TABLE projection_replay_large_docs (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            let projection = canonical_test_collection(&cassie, "projection_replay_large_docs");
            let mut csv = String::new();
            for index in 0..600 {
                use std::fmt::Write as _;
                writeln!(csv, "bulk-{index},title-{index},{index}").unwrap();
            }
            cassie
                .copy_from_csv_stdin(
                    &session,
                    &CopyStatement {
                        table: projection.clone(),
                        columns: vec!["_id".to_string(), "title".to_string(), "score".to_string()],
                        format: CopyFormat::Csv,
                        header: false,
                    },
                    csv.as_bytes(),
                )
                .unwrap();

            // Act
            cassie
                .replay_projection_batch(ProjectionReplayBatch {
                    projection,
                    source_identity: "large-replay-stream".to_string(),
                    batch_id: "large-replay-batch".to_string(),
                    lag: 0,
                    events: vec![ProjectionReplayEvent {
                        event_id: "large-replay-event".to_string(),
                        checkpoint: "large-checkpoint".to_string(),
                        position: Some(1),
                        document_id: "large-replay-doc".to_string(),
                        payload: Some(serde_json::json!({
                            "title": "replayed",
                            "score": 601,
                        })),
                    }],
                })
                .unwrap();
            let metadata = cassie
                .catalog
                .get_projection_metadata("projection_replay_large_docs")
                .unwrap();
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM projection_replay_large_docs WHERE score = 601",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("replayed".to_string())]]
            );
            assert_eq!(
                metadata.hashes.root.state,
                ProjectionVerificationState::Stale
            );
            assert_eq!(metadata.hashes.root.row_count, 601);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_failed_freshness_for_out_of_order_replay() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_replay_out_of_order");
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
                "CREATE TABLE projection_replay_order_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_order_docs");
        cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-1".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-2".to_string(),
                    checkpoint: "checkpoint-2".to_string(),
                    position: Some(2),
                    document_id: "doc-2".to_string(),
                    payload: Some(serde_json::json!({"title": "bravo"})),
                }],
            })
            .unwrap();

        // Act
        let error = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-2".to_string(),
                lag: 1,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-1".to_string(),
                    checkpoint: "checkpoint-1".to_string(),
                    position: Some(1),
                    document_id: "doc-1".to_string(),
                    payload: Some(serde_json::json!({"title": "alpha"})),
                }],
            })
            .unwrap_err();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT freshness, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert!(error.to_string().contains("out-of-order"));
        assert_eq!(checkpoint.rows.len(), 1);
        assert_eq!(checkpoint.rows[0][0], Value::String("failed".to_string()));
        assert!(matches!(&checkpoint.rows[0][1], Value::String(message) if message.contains("out-of-order")));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fail_duplicate_event_id_in_batch_without_partial_replay() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_replay_duplicate_in_batch");
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
                "CREATE TABLE projection_replay_conflict_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_replay_conflict_docs");

        // Act
        let error = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: projection.clone(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-conflict".to_string(),
                lag: 2,
                events: vec![
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-1".to_string(),
                        position: Some(1),
                        document_id: "doc-1".to_string(),
                        payload: Some(serde_json::json!({"title": "alpha"})),
                    },
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-2".to_string(),
                        position: Some(2),
                        document_id: "doc-2".to_string(),
                        payload: Some(serde_json::json!({"title": "bravo"})),
                    },
                ],
            })
            .unwrap_err();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_conflict_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT freshness, replay_batch_id, source_checkpoint, last_applied_event_id, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let restarted_checkpoint = restarted
            .execute_sql(
                &restarted_session,
                &format!(
                    "SELECT freshness, replay_batch_id, source_checkpoint, last_applied_event_id, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert!(error.to_string().contains("duplicate projection replay event"));
        assert_eq!(selected.rows, Vec::<Vec<Value>>::new());
        assert_eq!(checkpoint.rows.len(), 1);
        assert_eq!(checkpoint.rows[0][0], Value::String("failed".to_string()));
        assert_eq!(
            checkpoint.rows[0][1],
            Value::String("batch-conflict".to_string())
        );
        assert_eq!(checkpoint.rows[0][2], Value::String(String::new()));
        assert_eq!(checkpoint.rows[0][3], Value::String(String::new()));
        assert!(matches!(&checkpoint.rows[0][4], Value::String(message) if message.contains("duplicate projection replay event")));
        assert_eq!(restarted_checkpoint.rows, checkpoint.rows);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_refresh_materialized_projection_after_source_write() {
        // Arrange
        use_local_storage();
        let path = data_dir("materialized_projection_refresh");
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
                "CREATE TABLE projection_source_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title, score) VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_ready AS SELECT title, score FROM projection_source_docs WHERE score > 1",
                vec![],
            )
            .unwrap();
        let projection = cassie
            .catalog
            .get_materialized_projection("projection_ready")
            .expect("materialized projection metadata")
            .collection;
        let initial = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM projection_ready ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title, score) VALUES ('charlie', 3)",
                vec![],
            )
            .unwrap();
        let stale = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state FROM pg_catalog.pg_materialized_projections WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_ready",
                vec![],
            )
            .unwrap();
        let refreshed = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_ready ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            initial.rows,
            vec![vec![Value::String("bravo".to_string()), Value::Int64(2)]]
        );
        assert_eq!(stale.rows, vec![vec![Value::String("stale".to_string())]]);
        assert_eq!(
            refreshed.rows,
            vec![
                vec![Value::String("bravo".to_string())],
                vec![Value::String("charlie".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_dml_against_materialized_projection_output() {
        // Arrange
        use_local_storage();
        let path = data_dir("materialized_projection_read_only");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE projection_ro_docs (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_ro_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_ro AS SELECT title FROM projection_ro_docs",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_ro (title) VALUES ('bravo')",
                vec![],
            )
            .unwrap_err();

        // Assert
        assert!(error.to_string().contains("read-only"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_activate_built_materialized_projection_version() {
        // Arrange
        use_local_storage();
        let path = data_dir("materialized_projection_versions");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            let projection = seed_materialized_projection_version_fixture(&cassie, &session);

            // Act
            execute_statement(
                &cassie,
                &session,
                "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
            );
            let before_swap = query_rows(
                &cassie,
                &session,
                "SELECT title FROM projection_versioned ORDER BY title",
            );
            execute_statement(
                &cassie,
                &session,
                "ALTER MATERIALIZED PROJECTION projection_versioned ACTIVATE VERSION v2",
            );
            let after_swap = query_rows(
                &cassie,
                &session,
                "SELECT title FROM projection_versioned ORDER BY title",
            );
            execute_statement(
                &cassie,
                &session,
                "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v1",
            );
            let versions = projection_version_rows(&cassie, &session, &projection);

            // Assert
            assert_eq!(before_swap, vec![vec![Value::String("alpha".to_string())]]);
            assert_eq!(
                after_swap,
                vec![
                    vec![Value::String("alpha".to_string())],
                    vec![Value::String("bravo".to_string())],
                ]
            );
            assert_eq!(
                versions,
                vec![vec![
                    Value::String("v2".to_string()),
                    Value::String("active".to_string())
                ]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_materialized_projection_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("materialized_projection_restart");
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
                "CREATE TABLE projection_restart_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_restart_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_restart AS SELECT title FROM projection_restart_docs",
                vec![],
            )
            .unwrap();
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT title FROM projection_restart ORDER BY title",
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
}

// Formerly tests/projection_repair.rs.
mod projection_repair {
    use cassie::app::{
        Cassie, CassieSession, CassieSnapshotOptions, ProjectionManifestExportOptions,
    };
    use cassie::midge::adapter::{
        decode_column_batch_manifest_for_test, RowHashRecord, StorageFamily,
    };
    use cassie::sql::ast::{ProjectionRepairScope, QueryStatement};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    fn corrupt_first_row_hash(cassie: &Cassie, collection: &str) {
        let collection = canonical_test_collection(cassie, collection);
        let mut row_hash = cassie.midge.list_row_hashes(&collection).unwrap()[0].clone();
        let key = row_hash_storage_key(cassie, &collection, &row_hash.row_id);
        row_hash.state = cassie::midge::adapter::StoredHashState::Stale;
        let mut tx = cassie
            .midge
            .data_tx(cntryl_midge::TransactionMode::ReadWrite)
            .unwrap();
        tx.put(key, serde_json::to_vec(&row_hash).unwrap(), None)
            .unwrap();
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
    }

    fn row_hash_storage_key(cassie: &Cassie, collection: &str, row_id: &str) -> Vec<u8> {
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap()
            .into_iter()
            .find_map(|(key, value)| {
                let record = serde_json::from_slice::<RowHashRecord>(&value).ok()?;
                (record.collection == collection && record.row_id == row_id).then_some(key)
            })
            .expect("row hash key should exist")
    }

    fn delete_column_batch_metadata(cassie: &Cassie, collection: &str, index_name: &str) {
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap();
        let mut tx = cassie
            .midge
            .data_tx(cntryl_midge::TransactionMode::ReadWrite)
            .unwrap();
        for (key, value) in entries {
            let Ok(metadata) = decode_column_batch_manifest_for_test(&value) else {
                continue;
            };
            if metadata.collection == collection && metadata.index_name == index_name {
                tx.delete(key).unwrap();
            }
        }
        tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
    }

    fn seed_full_rebuild_fixture(cassie: &Cassie, session: &CassieSession) -> String {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE repair_full_rebuild_source (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "INSERT INTO repair_full_rebuild_source (title, score) VALUES ('alpha', 1)",
                vec![],
            )
            .unwrap();
        cassie
        .execute_sql(
            session,
            "CREATE MATERIALIZED PROJECTION repair_full_rebuild AS SELECT title, score FROM repair_full_rebuild_source",
            vec![],
        )
        .unwrap();
        let metadata = cassie
            .catalog
            .get_materialized_projection("repair_full_rebuild")
            .expect("materialized projection metadata");
        let projection = metadata.collection.clone();
        let output_collection = metadata
            .materialized
            .as_ref()
            .expect("materialized projection definition")
            .output_collection
            .clone();
        corrupt_first_row_hash(cassie, &output_collection);
        projection
    }

    fn seed_projection_version_fixture(
        cassie: &Cassie,
        session: &CassieSession,
    ) -> (String, String) {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE repair_projection_version_source (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "INSERT INTO repair_projection_version_source (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
        .execute_sql(
            session,
            "CREATE MATERIALIZED PROJECTION repair_projection_version AS SELECT title FROM repair_projection_version_source",
            vec![],
        )
        .unwrap();
        cassie
            .execute_sql(
                session,
                "INSERT INTO repair_projection_version_source (title) VALUES ('bravo')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "ALTER MATERIALIZED PROJECTION repair_projection_version BUILD VERSION",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .catalog
            .get_materialized_projection("repair_projection_version")
            .expect("materialized projection metadata");
        let projection = metadata.collection.clone();
        let output_collection = metadata
            .versions
            .iter()
            .find(|version| version.version_id == "v2")
            .expect("v2 projection metadata")
            .output_collection
            .clone();
        (projection, output_collection)
    }

    #[test]
    fn should_parse_projection_repair_commands() {
        // Arrange
        let plan_sql = "PLAN REPAIR PROJECTION repair_docs VERSION v2 SCOPE range";
        let repair_sql = "REPAIR PROJECTION repair_docs SCOPE full-rebuild";

        // Act
        let plan = cassie::sql::parse_statement(plan_sql).unwrap();
        let repair = cassie::sql::parse_statement(repair_sql).unwrap();

        // Assert
        let QueryStatement::PlanRepairProjection(plan) = plan.statement else {
            panic!("expected PLAN REPAIR PROJECTION");
        };
        assert_eq!(plan.target.name, "repair_docs");
        assert_eq!(plan.target.version_id.as_deref(), Some("v2"));
        assert_eq!(plan.scope, ProjectionRepairScope::Range);

        let QueryStatement::RepairProjection(repair) = repair.statement else {
            panic!("expected REPAIR PROJECTION");
        };
        assert_eq!(repair.target.name, "repair_docs");
        assert_eq!(repair.scope, ProjectionRepairScope::FullRebuild);
    }

    #[test]
    fn should_plan_dry_run_repair_from_integrity_findings() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_plan");
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
                    "CREATE TABLE repair_plan_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO repair_plan_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            let projection = canonical_test_collection(&cassie, "repair_plan_docs");
            corrupt_first_row_hash(&cassie, "repair_plan_docs");
            cassie
                .execute_sql(
                    &session,
                    "VERIFY PROJECTION repair_plan_docs MODE hashes_only",
                    vec![],
                )
                .unwrap();

            // Act
            let scopes = [
                ("row", "rebuild_projection_hashes", true),
                ("range", "rebuild_projection_hashes", true),
                ("index", "rebuild_index_entries", false),
                ("projection-version", "refresh_projection_version", false),
                ("full-rebuild", "refresh_materialized_projection", false),
            ];
            let plans = scopes
                .into_iter()
                .map(|(scope, action, executable)| {
                    let plan = cassie
                        .execute_sql(
                            &session,
                            &format!("PLAN REPAIR PROJECTION repair_plan_docs SCOPE {scope}"),
                            vec![],
                        )
                        .unwrap();
                    (scope, action, executable, plan)
                })
                .collect::<Vec<_>>();

            // Assert
            for (scope, action, executable, plan) in plans {
                assert_eq!(plan.command, "PLAN REPAIR PROJECTION");
                assert_eq!(plan.rows[0][0], Value::String("planned".to_string()));
                assert_eq!(plan.rows[0][2], Value::String(scope.replace('-', "_")));
                assert_eq!(plan.rows[0][4], Value::String(action.to_string()));
                assert_eq!(plan.rows[0][5], Value::Bool(executable));
                assert_eq!(
                    plan.rows[0][6],
                    Value::String(format!("VERIFY PROJECTION {projection} MODE full"))
                );
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_verified_local_hash_repair_with_audit() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_execute");
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
                "CREATE TABLE repair_execute_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO repair_execute_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "repair_execute_docs");
        corrupt_first_row_hash(&cassie, "repair_execute_docs");
        cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_execute_docs MODE hashes_only",
                vec![],
            )
            .unwrap();

        // Act
        let repair = cassie
            .execute_sql(
                &session,
                "REPAIR PROJECTION repair_execute_docs SCOPE row",
                vec![],
            )
            .unwrap();
        let audit = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, scope, action, post_verification_state FROM pg_catalog.pg_projection_repair_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(repair.command, "REPAIR PROJECTION");
        assert_eq!(repair.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(
            repair.rows[0][7],
            Value::String("verified".to_string())
        );
        assert_eq!(audit.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(audit.rows[0][1], Value::String("row".to_string()));
        assert_eq!(
            audit.rows[0][2],
            Value::String("rebuild_projection_hashes".to_string())
        );
        assert_eq!(audit.rows[0][3], Value::String("verified".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_repair_large_projection_hash_manifest_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_large_manifest");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE repair_large_manifest_docs (title TEXT, score INT)",
                vec![],
            )
            .expect("create table");
        let projection = canonical_test_collection(&cassie, "repair_large_manifest_docs");
        let documents = (0..1_024)
            .map(|index| {
                (
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({"title": format!("title-{index}"), "score": index}),
                )
            })
            .collect();
        cassie
            .midge
            .put_documents(&projection, documents)
            .expect("seed large manifest");
        cassie
            .midge
            .rebuild_projection_hashes(&projection)
            .expect("build initial ranges");
        corrupt_first_row_hash(&cassie, &projection);

        // Act
        let verification = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_large_manifest_docs MODE hashes_only",
                vec![],
            )
            .expect("verify large manifest");
        let plan = cassie
            .execute_sql(
                &session,
                "PLAN REPAIR PROJECTION repair_large_manifest_docs SCOPE range",
                vec![],
            )
            .expect("plan range repair");
        let repair = cassie
            .execute_sql(
                &session,
                "REPAIR PROJECTION repair_large_manifest_docs SCOPE range",
                vec![],
            )
            .expect("repair large manifest");
        let mut options = ProjectionManifestExportOptions::for_instance("large-manifest");
        options.generated_ms = Some(4_000_000_000_000);
        options.include_row_hashes = true;
        let manifest = cassie
            .export_projection_verification_manifest(&projection, options)
            .expect("export repaired manifest");
        let root = cassie
            .midge
            .root_hash(&projection)
            .expect("read repaired root")
            .expect("repaired root");

        // Assert
        assert_eq!(verification.rows[0][0], Value::String("failed".to_string()));
        assert_eq!(verification.rows[0][5], Value::Int64(1));
        assert_eq!(plan.rows[0][5], Value::Bool(true));
        assert_eq!(repair.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(repair.rows[0][7], Value::String("verified".to_string()));
        assert_eq!(root.row_count, 1_024);
        assert_eq!(root.range_count, 4);
        assert_eq!(
            manifest.root.as_ref().expect("manifest root").row_count,
            1_024
        );
        assert_eq!(manifest.ranges.len(), 4);
        assert_eq!(manifest.row_hashes.len(), 1_024);
        assert_eq!(
            manifest
                .ranges
                .iter()
                .map(|range| range.row_count)
                .sum::<u64>(),
            1_024
        );
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("restart cassie");
        restarted.startup().expect("restart startup");
        let restarted_root = restarted
            .midge
            .root_hash(&projection)
            .expect("read root after restart")
            .expect("root after restart");
        assert_eq!(
            restarted_root.built_generation,
            restarted.midge.collection_generation(&projection).unwrap()
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_execute_verified_column_index_repair_with_audit() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_index_execute");
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
                    "CREATE TABLE repair_index_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO repair_index_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX repair_index_docs_idx ON repair_index_docs USING column (title)",
                    vec![],
                )
                .unwrap();
            let collection = canonical_test_collection(&cassie, "repair_index_docs");
            let index_name = cassie
                .midge
                .get_index(&collection, "repair_index_docs_idx")
                .unwrap()
                .unwrap()
                .name;
            delete_column_batch_metadata(&cassie, &collection, &index_name);

            // Act
            let verification = cassie
                .execute_sql(
                    &session,
                    "VERIFY PROJECTION repair_index_docs MODE indexes_only",
                    vec![],
                )
                .unwrap();
            let plan = cassie
                .execute_sql(
                    &session,
                    "PLAN REPAIR PROJECTION repair_index_docs SCOPE index",
                    vec![],
                )
                .unwrap();
            let repair = cassie
                .execute_sql(
                    &session,
                    "REPAIR PROJECTION repair_index_docs SCOPE index",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(verification.rows[0][0], Value::String("failed".to_string()));
            assert_eq!(verification.rows[0][4], Value::Int64(1));
            assert_eq!(verification.rows[0][6], Value::Bool(true));
            assert_eq!(plan.rows[0][5], Value::Bool(true));
            assert_eq!(repair.rows[0][0], Value::String("completed".to_string()));
            assert_eq!(repair.rows[0][7], Value::String("verified".to_string()));
            assert!(cassie
                .midge
                .get_column_batch_metadata(&collection, &index_name)
                .unwrap()
                .is_some());
            drop(cassie);
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            assert!(restarted
                .midge
                .get_column_batch_metadata(&collection, &index_name)
                .unwrap()
                .is_some());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_verified_full_rebuild_repair_with_audit() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_full_rebuild_execute");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let projection = seed_full_rebuild_fixture(&cassie, &session);

        // Act
        let verification = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_full_rebuild MODE full",
                vec![],
            )
            .unwrap();
        let plan = cassie
            .execute_sql(
                &session,
                "PLAN REPAIR PROJECTION repair_full_rebuild SCOPE full-rebuild",
                vec![],
            )
            .unwrap();
        let repair = cassie
            .execute_sql(
                &session,
                "REPAIR PROJECTION repair_full_rebuild SCOPE full-rebuild",
                vec![],
            )
            .unwrap();
        let audit = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, scope, action, post_verification_state FROM pg_catalog.pg_projection_repair_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM repair_full_rebuild ORDER BY title",
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let restarted_selected = restarted
            .execute_sql(
                &restarted_session,
                "SELECT title, score FROM repair_full_rebuild ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(verification.rows[0][0], Value::String("failed".to_string()));
        assert_eq!(verification.rows[0][6], Value::Bool(true));
        assert_eq!(plan.rows[0][5], Value::Bool(true));
        assert_eq!(repair.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(repair.rows[0][7], Value::String("verified".to_string()));
        assert_eq!(audit.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(audit.rows[0][1], Value::String("full_rebuild".to_string()));
        assert_eq!(
            audit.rows[0][2],
            Value::String("refresh_materialized_projection".to_string())
        );
        assert_eq!(audit.rows[0][3], Value::String("verified".to_string()));
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(1)]]
        );
        assert_eq!(restarted_selected.rows, selected.rows);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_verified_projection_version_repair_without_activation() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_version_execute");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let (projection, output_collection) = seed_projection_version_fixture(&cassie, &session);
        corrupt_first_row_hash(&cassie, &output_collection);
        let verify_sql = "VERIFY PROJECTION repair_projection_version VERSION v2 MODE full";

        // Act
        let verification = cassie.execute_sql(&session, verify_sql, vec![]).unwrap();
        let plan = cassie
            .execute_sql(
                &session,
                "PLAN REPAIR PROJECTION repair_projection_version VERSION v2 SCOPE projection-version",
                vec![],
            )
            .unwrap();
        let repair = cassie
            .execute_sql(
                &session,
                "REPAIR PROJECTION repair_projection_version VERSION v2 SCOPE projection-version",
                vec![],
            )
            .unwrap();
        let audit = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, scope, action, post_verification_state FROM pg_catalog.pg_projection_repair_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        let versions = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT version_id, state FROM pg_catalog.pg_projection_versions WHERE projection_name = '{projection}' ORDER BY version_id"
                ),
                vec![],
            )
            .unwrap();
        let active_rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM repair_projection_version ORDER BY title",
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let restarted_verification = restarted
            .execute_sql(&restarted_session, verify_sql, vec![])
            .unwrap();
        let restarted_active_rows = restarted
            .execute_sql(
                &restarted_session,
                "SELECT title FROM repair_projection_version ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(verification.rows[0][0], Value::String("failed".to_string()));
        assert_eq!(verification.rows[0][6], Value::Bool(true));
        assert_eq!(plan.rows[0][5], Value::Bool(true));
        assert_eq!(repair.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(repair.rows[0][7], Value::String("verified".to_string()));
        assert_eq!(audit.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(audit.rows[0][1], Value::String("projection_version".to_string()));
        assert_eq!(
            audit.rows[0][2],
            Value::String("refresh_projection_version".to_string())
        );
        assert_eq!(audit.rows[0][3], Value::String("verified".to_string()));
        assert_eq!(
            versions.rows,
            vec![
                vec![
                    Value::String("v1".to_string()),
                    Value::String("active".to_string())
                ],
                vec![
                    Value::String("v2".to_string()),
                    Value::String("built".to_string())
                ]
            ]
        );
        assert_eq!(active_rows.rows, vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(
            restarted_verification.rows[0][0],
            Value::String("verified".to_string())
        );
        assert_eq!(restarted_active_rows.rows, active_rows.rows);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_rehearse_snapshot_rollback_after_projection_version_repair() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_version_rollback");
        let snapshot = data_dir("projection_repair_version_rollback_snapshot");
        let restored = data_dir("projection_repair_version_rollback_restored");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let (projection, output_collection) = seed_projection_version_fixture(&cassie, &session);
        drop(cassie);
        Cassie::create_snapshot_from_data_dir(
            &path,
            &snapshot,
            CassieSnapshotOptions {
                generated_ms: Some(8_642),
            },
        )
        .unwrap();

        // Act
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        corrupt_first_row_hash(&cassie, &output_collection);
        let failed = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_projection_version VERSION v2 MODE full",
                vec![],
            )
            .unwrap();
        let repaired = cassie
            .execute_sql(
                &session,
                "REPAIR PROJECTION repair_projection_version VERSION v2 SCOPE projection-version",
                vec![],
            )
            .unwrap();
        drop(cassie);
        let manifest = Cassie::restore_snapshot(&snapshot, &restored).unwrap();
        let rolled_back = Cassie::new_with_data_dir(&restored).unwrap();
        rolled_back.startup().unwrap();
        let rolled_back_session = rolled_back.create_session("tester", None);
        let verification = rolled_back
            .execute_sql(
                &rolled_back_session,
                "VERIFY PROJECTION repair_projection_version VERSION v2 MODE full",
                vec![],
            )
            .unwrap();
        let active_rows = rolled_back
            .execute_sql(
                &rolled_back_session,
                "SELECT title FROM repair_projection_version ORDER BY title",
                vec![],
            )
            .unwrap();
        let reports = rolled_back
            .execute_sql(
                &rolled_back_session,
                &format!(
                    "SELECT state FROM pg_catalog.pg_projection_repair_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(failed.rows[0][0], Value::String("failed".to_string()));
        assert_eq!(repaired.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(manifest.generated_ms, 8_642);
        assert_eq!(verification.rows[0][0], Value::String("verified".to_string()));
        assert_eq!(active_rows.rows, vec![vec![Value::String("alpha".to_string())]]);
        assert!(reports.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
        let _ = std::fs::remove_dir_all(snapshot);
        let _ = std::fs::remove_dir_all(restored);
    });
    }

    #[test]
    fn should_reject_unsafe_repair_scope_deterministically() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_unsafe");
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
                    "CREATE TABLE repair_unsafe_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO repair_unsafe_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "VERIFY PROJECTION repair_unsafe_docs MODE full",
                    vec![],
                )
                .unwrap();

            // Act
            let error = cassie
                .execute_sql(
                    &session,
                    "REPAIR PROJECTION repair_unsafe_docs SCOPE index",
                    vec![],
                )
                .expect_err("index repair should require index-specific findings");

            // Assert
            assert!(error
                .to_string()
                .contains("repair scope 'index' is not executable"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_not_run_repair_from_query_path() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_repair_query_path");
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
                "CREATE TABLE repair_query_path_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO repair_query_path_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "repair_query_path_docs");
        corrupt_first_row_hash(&cassie, "repair_query_path_docs");
        cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_query_path_docs MODE hashes_only",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM repair_query_path_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let reports = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state FROM pg_catalog.pg_projection_repair_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert!(reports.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/projection_verification.rs.
mod projection_verification {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::catalog::ProjectionVerificationState;
    use cassie::midge::adapter::{RowHashRecord, StorageFamily};
    use cassie::sql::ast::{ProjectionVerificationMode, QueryStatement};
    use cassie::types::Value;
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_parse_verify_projection_command() {
        // Arrange
        let sql = "VERIFY PROJECTION projection_docs VERSION v2 MODE hashes-only";

        // Act
        let parsed = cassie::sql::parse_statement(sql).unwrap();

        // Assert
        let QueryStatement::VerifyProjection(statement) = parsed.statement else {
            panic!("expected VERIFY PROJECTION");
        };
        assert_eq!(statement.name, "projection_docs");
        assert_eq!(statement.version_id.as_deref(), Some("v2"));
        assert_eq!(statement.mode, ProjectionVerificationMode::HashesOnly);
    }

    #[test]
    fn should_keep_row_hashes_deterministic_across_restart_schema_epoch_changes() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_row_hash_deterministic");
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
                    "CREATE TABLE projection_hash_docs (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO projection_hash_docs (title, score) VALUES ('alpha', 1)",
                    vec![],
                )
                .unwrap();
            let collection = canonical_test_collection(&cassie, "projection_hash_docs");
            let row_id = cassie.midge.scan_documents(&collection).unwrap()[0]
                .id
                .clone();
            let first = cassie
                .midge
                .row_hash(&collection, &row_id)
                .unwrap()
                .unwrap();
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let restarted_hash = restarted
                .midge
                .row_hash(&collection, &row_id)
                .unwrap()
                .unwrap();

            // Act
            restarted
                .execute_sql(
                    &restarted.create_session("tester", None),
                    "ALTER TABLE projection_hash_docs ADD COLUMN summary TEXT",
                    vec![],
                )
                .unwrap();
            let schema_hash = restarted
                .midge
                .row_hash(&collection, &row_id)
                .unwrap()
                .unwrap();
            restarted
                .execute_sql(
                    &restarted.create_session("tester", None),
                    "UPDATE projection_hash_docs SET title = 'beta'",
                    vec![],
                )
                .unwrap();
            let updated_hash = restarted
                .midge
                .row_hash(&collection, &row_id)
                .unwrap()
                .unwrap();

            // Assert
            assert_eq!(first.digest, restarted_hash.digest);
            assert_ne!(first.digest, schema_hash.digest);
            assert_ne!(schema_hash.digest, updated_hash.digest);
            assert_eq!(updated_hash.algorithm, "cassie-fnv128");
            assert_eq!(updated_hash.digest_length, 16);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_empty_projection_root_after_delete() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_row_hash_delete");
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
                    "CREATE TABLE projection_delete_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO projection_delete_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            let row_id = cassie
                .midge
                .scan_documents(&canonical_test_collection(
                    &cassie,
                    "projection_delete_docs",
                ))
                .unwrap()[0]
                .id
                .clone();
            let collection = canonical_test_collection(&cassie, "projection_delete_docs");

            // Act
            cassie
                .execute_sql(&session, "DELETE FROM projection_delete_docs", vec![])
                .unwrap();
            let row_hash = cassie.midge.row_hash(&collection, &row_id).unwrap();
            let root = cassie.midge.root_hash(&collection).unwrap().unwrap();

            // Assert
            assert!(row_hash.is_none());
            assert_eq!(root.row_count, 0);
            assert_eq!(root.state, cassie::midge::adapter::StoredHashState::Empty);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_expose_projection_verification_state_through_catalog_views() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_hash_catalog_views");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE projection_view_docs (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_view_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "projection_view_docs");

        // Act
        let verified = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION projection_view_docs MODE full",
                vec![],
            )
            .unwrap();
        let hashes = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT row_state, row_count, range_count, root_state FROM pg_catalog.pg_projection_hashes WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        let operations = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT freshness, verification_state, root_state FROM pg_catalog.pg_projection_operations WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();
        let reports = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, mode, mismatch_count, missing_count, stale_count FROM pg_catalog.pg_projection_integrity_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(verified.rows[0][0], Value::String("verified".to_string()));
        assert_eq!(
            hashes.rows,
            vec![vec![
                Value::String("current".to_string()),
                Value::Int64(1),
                Value::Int64(1),
                Value::String("current".to_string()),
            ]]
        );
        assert_eq!(
            operations.rows,
            vec![vec![
                Value::String("unknown".to_string()),
                Value::String("unknown".to_string()),
                Value::String("current".to_string()),
            ]]
        );
        assert_eq!(
            reports.rows,
            vec![vec![
                Value::String("verified".to_string()),
                Value::String("full".to_string()),
                Value::Int64(0),
                Value::Int64(0),
                Value::Int64(0),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_report_integrity_failure_for_corrupt_row_hash() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_hash_corruption");
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
                    "CREATE TABLE projection_corrupt_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO projection_corrupt_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            let collection = canonical_test_collection(&cassie, "projection_corrupt_docs");
            let mut row_hash = cassie.midge.list_row_hashes(&collection).unwrap()[0].clone();
            let row_hash_key = row_hash_storage_key(&cassie, &collection, &row_hash.row_id);
            row_hash.digest = "00000000000000000000000000000000".to_string();
            let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
            tx.put(row_hash_key, serde_json::to_vec(&row_hash).unwrap(), None)
                .unwrap();
            tx.commit(WriteOptions::sync()).unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "VERIFY PROJECTION projection_corrupt_docs MODE hashes_only",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows[0][0], Value::String("failed".to_string()));
            let Value::Int64(mismatches) = result.rows[0][3] else {
                panic!("expected mismatch count");
            };
            assert!(mismatches >= 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    fn row_hash_storage_key(cassie: &Cassie, collection: &str, row_id: &str) -> Vec<u8> {
        let collection = canonical_test_collection(cassie, collection);
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap()
            .into_iter()
            .find_map(|(key, value)| {
                let record = serde_json::from_slice::<RowHashRecord>(&value).ok()?;
                (record.collection == collection && record.row_id == row_id).then_some(key)
            })
            .expect("row hash key should exist")
    }

    #[test]
    fn should_block_unverified_projection_version_activation_without_unsafe_override() {
        // Arrange
        use_local_storage();
        let path = data_dir("projection_activation_verification");
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
                "CREATE TABLE projection_source_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_verified AS SELECT title FROM projection_source_docs",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED PROJECTION projection_verified BUILD VERSION",
                vec![],
            )
            .unwrap();
        let mut metadata = cassie
            .catalog
            .get_materialized_projection("projection_verified")
            .unwrap();
        let target = metadata
            .versions
            .iter_mut()
            .find(|version| version.version_id == "v2")
            .unwrap();
        target.verification.state = ProjectionVerificationState::Failed;
        target.verification.failure_reason = Some("test corruption".to_string());
        cassie.midge.put_projection_metadata(&metadata).unwrap();
        cassie.catalog.register_projection_metadata(metadata);

        // Act
        let blocked = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_verified ACTIVATE VERSION v2",
            vec![],
        );
        let unsafe_result = cassie.execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION projection_verified ACTIVATE VERSION v2 UNSAFE",
            vec![],
        );

        // Assert
        assert!(blocked.is_err());
        assert!(unsafe_result.is_ok());

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/read_model_bounded_join_selection.rs.
mod read_model_bounded_join_selection {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::catalog::{canonical_relation_name, CollectionCardinalityStats};
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    fn vectorized_join_config() -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.vectorized_joins_enabled = true;
        config.limits.vectorized_join_batch_size = 8;
        config.limits.query_memory_budget_bytes = 1024 * 1024;
        config.limits.adaptive_execution_enabled = false;
        config.limits.operator_switching_enabled =
            cassie::config::OperatorSwitchingEnabled::disabled();
        config
    }

    fn vectorized_join_budget_config(query_memory_budget_bytes: usize) -> CassieRuntimeConfig {
        let mut config = vectorized_join_config();
        config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        config
    }

    fn canonical_collection(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    fn hydrate_cardinality(cassie: &Cassie, collection: &str) {
        let collection = canonical_collection(collection);
        let stats = cassie
            .midge
            .rebuild_cardinality_stats_for_collection(&collection)
            .expect("cardinality stats");
        cassie.catalog.hydrate_cardinality_stats(&collection, stats);
    }

    fn hydrate_row_count_only(cassie: &Cassie, collection: &str, row_count: u64) {
        let collection = canonical_collection(collection);
        cassie.catalog.hydrate_cardinality_stats(
            &collection,
            CollectionCardinalityStats {
                row_count,
                ..CollectionCardinalityStats::default()
            },
        );
    }

    fn clear_cardinality(cassie: &Cassie, collection: &str) {
        cassie
            .catalog
            .clear_cardinality_stats(&canonical_collection(collection));
    }

    fn metric_delta(after: &serde_json::Value, before: &serde_json::Value, path: &[&str]) -> u64 {
        let after_value = path.iter().fold(after, |value, key| &value[*key]);
        let before_value = path.iter().fold(before, |value, key| &value[*key]);
        after_value.as_u64().unwrap() - before_value.as_u64().unwrap()
    }

    fn create_join_tables(
        cassie: &Cassie,
        session: &cassie::app::CassieSession,
        users_table: &str,
        orders_table: &str,
    ) {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {users_table} (user_key INT, name TEXT)"),
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {orders_table} (order_user_key INT, total INT)"),
                vec![],
            )
            .unwrap();
    }

    fn put_users(cassie: &Cassie, table: &str, count: usize) {
        let table = canonical_collection(table);
        let users = (0..count)
            .map(|index| {
                (
                    Some(format!("user-{index:03}")),
                    serde_json::json!({
                        "user_key": i64::try_from(index).unwrap(),
                        "name": format!("user-{index:03}"),
                    }),
                )
            })
            .collect();
        cassie.midge.put_fresh_documents(&table, users).unwrap();
    }

    fn put_users_with_keys(cassie: &Cassie, table: &str, keys: &[i64]) {
        let table = canonical_collection(table);
        let users = keys
            .iter()
            .enumerate()
            .map(|(index, key)| {
                (
                    Some(format!("user-{index:04}")),
                    serde_json::json!({
                        "user_key": *key,
                        "name": format!("user-{index:04}"),
                    }),
                )
            })
            .collect();
        cassie.midge.put_fresh_documents(&table, users).unwrap();
    }

    fn put_orders(cassie: &Cassie, table: &str, keys: &[i64]) {
        let table = canonical_collection(table);
        let orders = keys
            .iter()
            .enumerate()
            .map(|(index, key)| {
                (
                    Some(format!("order-{index:03}")),
                    serde_json::json!({
                        "order_user_key": *key,
                        "total": *key,
                    }),
                )
            })
            .collect();
        cassie.midge.put_fresh_documents(&table, orders).unwrap();
    }

    fn select_inner_sql(users_table: &str, orders_table: &str, limit: usize) -> String {
        format!(
            "SELECT {users_table}.name, {orders_table}.total \
         FROM {users_table} JOIN {orders_table} \
         ON {users_table}.user_key = {orders_table}.order_user_key \
         LIMIT {limit}"
        )
    }

    #[test]
    fn should_sample_row_count_stats_to_reduce_larger_bounded_join_without_field_stats() {
        // Arrange
        use_local_storage();
        let path = data_dir("row_count_sample_bounded_inner_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(
                &cassie,
                &session,
                "row_count_sample_users",
                "row_count_sample_orders",
            );
            let user_keys = (0..1_000)
                .map(|index| i64::from(index % 10))
                .collect::<Vec<_>>();
            let order_keys = (0..3_000)
                .map(|index| i64::from(index % 10))
                .collect::<Vec<_>>();
            put_users_with_keys(&cassie, "row_count_sample_users", &user_keys);
            put_orders(&cassie, "row_count_sample_orders", &order_keys);
            hydrate_row_count_only(&cassie, "row_count_sample_users", 1_000);
            hydrate_row_count_only(&cassie, "row_count_sample_orders", 3_000);
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql("row_count_sample_users", "row_count_sample_orders", 500),
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 500);
            assert_eq!(
                metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]),
                1_000
            );
            assert!(metric_delta(&after, &before, &["joins", "vectorized_probe_rows_total"]) <= 5);
            let expected_orders = canonical_collection("row_count_sample_orders");
            assert_eq!(
                after["read_paths"]["last_collection_scan_collection"].as_str(),
                Some(expected_orders.as_str())
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_use_fanout_stats_to_reduce_larger_bounded_join_build_side() {
        // Arrange
        use_local_storage();
        let path = data_dir("fanout_stats_bounded_inner_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(
                &cassie,
                &session,
                "fanout_small_users",
                "fanout_large_orders",
            );
            let user_keys = (0..1_000)
                .map(|index| i64::from(index % 10))
                .collect::<Vec<_>>();
            let order_keys = (0..3_000)
                .map(|index| i64::from(index % 10))
                .collect::<Vec<_>>();
            put_users_with_keys(&cassie, "fanout_small_users", &user_keys);
            put_orders(&cassie, "fanout_large_orders", &order_keys);
            hydrate_cardinality(&cassie, "fanout_small_users");
            hydrate_cardinality(&cassie, "fanout_large_orders");
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql("fanout_small_users", "fanout_large_orders", 500),
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 500);
            assert_eq!(
                metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]),
                1_000
            );
            assert!(metric_delta(&after, &before, &["joins", "vectorized_probe_rows_total"]) <= 5);
            let expected_orders = canonical_collection("fanout_large_orders");
            assert_eq!(
                after["read_paths"]["last_collection_scan_collection"].as_str(),
                Some(expected_orders.as_str())
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_probe_indexed_right_source_for_bounded_inner_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("right_indexed_bounded_inner_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(
                &cassie,
                &session,
                "bounded_right_idx_users",
                "bounded_right_idx_orders",
            );
            put_users(&cassie, "bounded_right_idx_users", 4);
            put_orders(&cassie, "bounded_right_idx_orders", &[0, 1, 2, 3]);
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX bounded_right_idx_orders_key_idx \
                 ON bounded_right_idx_orders USING btree (order_user_key)",
                    vec![],
                )
                .unwrap();
            hydrate_cardinality(&cassie, "bounded_right_idx_users");
            hydrate_cardinality(&cassie, "bounded_right_idx_orders");
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql("bounded_right_idx_users", "bounded_right_idx_orders", 2),
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("user-000".to_string()), Value::Int64(0)],
                    vec![Value::String("user-001".to_string()), Value::Int64(1)],
                ]
            );
            let expected_orders = canonical_collection("bounded_right_idx_orders");
            let expected_index = canonical_collection("bounded_right_idx_orders_key_idx");
            assert_eq!(
                after["read_paths"]["last_index_scan_collection"].as_str(),
                Some(expected_orders.as_str())
            );
            assert_eq!(
                after["read_paths"]["last_index_scan_index"].as_str(),
                Some(expected_index.as_str())
            );
            assert!(metric_delta(&after, &before, &["read_paths", "index_seek_scans"]) > 0);
            assert!(metric_delta(&after, &before, &["read_paths", "collection_scan_rows"]) <= 2);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_existing_left_index_probe_when_estimates_tie() {
        // Arrange
        use_local_storage();
        let path = data_dir("indexed_bounded_tie");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(&cassie, &session, "bounded_tie_users", "bounded_tie_orders");
            put_users(&cassie, "bounded_tie_users", 4);
            put_orders(&cassie, "bounded_tie_orders", &[0, 1, 2, 3]);
            for sql in [
                "CREATE INDEX bounded_tie_users_key_idx \
             ON bounded_tie_users USING btree (user_key)",
                "CREATE INDEX bounded_tie_orders_key_idx \
             ON bounded_tie_orders USING btree (order_user_key)",
            ] {
                cassie.execute_sql(&session, sql, vec![]).unwrap();
            }
            hydrate_cardinality(&cassie, "bounded_tie_users");
            hydrate_cardinality(&cassie, "bounded_tie_orders");

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql("bounded_tie_users", "bounded_tie_orders", 2),
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 2);
            let expected_users = canonical_collection("bounded_tie_users");
            let expected_index = canonical_collection("bounded_tie_users_key_idx");
            assert_eq!(
                metrics["read_paths"]["last_index_scan_collection"].as_str(),
                Some(expected_users.as_str())
            );
            assert_eq!(
                metrics["read_paths"]["last_index_scan_index"].as_str(),
                Some(expected_index.as_str())
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_select_smaller_build_side_for_late_match_bounded_inner_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("late_match_bounded_inner_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(&cassie, &session, "late_small_users", "late_large_orders");
            put_users(&cassie, "late_small_users", 2);
            let order_keys = std::iter::repeat_n(99_i64, 198)
                .chain([0_i64, 1_i64])
                .collect::<Vec<_>>();
            put_orders(&cassie, "late_large_orders", &order_keys);
            hydrate_cardinality(&cassie, "late_small_users");
            hydrate_cardinality(&cassie, "late_large_orders");
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql("late_small_users", "late_large_orders", 2),
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("user-000".to_string()), Value::Int64(0)],
                    vec![Value::String("user-001".to_string()), Value::Int64(1)],
                ]
            );
            assert!(metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]) <= 2);
            let expected_orders = canonical_collection("late_large_orders");
            assert_eq!(
                after["read_paths"]["last_collection_scan_collection"].as_str(),
                Some(expected_orders.as_str())
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_use_bounded_row_count_probe_when_cardinality_stats_are_missing() {
        // Arrange
        use_local_storage();
        let path = data_dir("missing_stats_bounded_row_count_probe");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(
                &cassie,
                &session,
                "missing_probe_users",
                "missing_probe_orders",
            );
            put_users(&cassie, "missing_probe_users", 2);
            let order_keys = std::iter::repeat_n(99_i64, 198)
                .chain([0_i64, 1_i64])
                .collect::<Vec<_>>();
            put_orders(&cassie, "missing_probe_orders", &order_keys);
            clear_cardinality(&cassie, "missing_probe_users");
            clear_cardinality(&cassie, "missing_probe_orders");
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql("missing_probe_users", "missing_probe_orders", 2),
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("user-000".to_string()), Value::Int64(0)],
                    vec![Value::String("user-001".to_string()), Value::Int64(1)],
                ]
            );
            assert!(metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]) <= 2);
            let expected_orders = canonical_collection("missing_probe_orders");
            assert_eq!(
                after["read_paths"]["last_collection_scan_collection"].as_str(),
                Some(expected_orders.as_str())
            );
            assert_eq!(
                after["joins"]["last_bounded_side_selection_reason"].as_str(),
                Some("left_build_bounded_row_count_probe")
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_existing_streaming_join_when_missing_stats_do_not_prove_smaller_left() {
        // Arrange
        use_local_storage();
        let path = data_dir("ambiguous_missing_stats_bounded_inner_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(
                &cassie,
                &session,
                "ambiguous_missing_stats_users",
                "ambiguous_missing_stats_orders",
            );
            put_users(&cassie, "ambiguous_missing_stats_users", 4);
            put_orders(
                &cassie,
                "ambiguous_missing_stats_orders",
                &[0, 1, 2, 3, 99, 99],
            );
            clear_cardinality(&cassie, "ambiguous_missing_stats_users");
            clear_cardinality(&cassie, "ambiguous_missing_stats_orders");
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql(
                        "ambiguous_missing_stats_users",
                        "ambiguous_missing_stats_orders",
                        2,
                    ),
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 2);
            assert_eq!(
                metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]),
                6
            );
            let expected_users = canonical_collection("ambiguous_missing_stats_users");
            assert_eq!(
                after["read_paths"]["last_collection_scan_collection"].as_str(),
                Some(expected_users.as_str())
            );
            assert_eq!(
                after["joins"]["last_bounded_side_selection_reason"].as_str(),
                Some("right_build_kept_unproven")
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_existing_streaming_join_when_left_probe_exceeds_temp_budget() {
        // Arrange
        use_local_storage();
        let path = data_dir("bounded_probe_temp_budget");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(
                &path,
                vectorized_join_budget_config(9 * 1024),
            )
            .unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(
                &cassie,
                &session,
                "budget_probe_users",
                "budget_probe_orders",
            );
            put_users(&cassie, "budget_probe_users", 20);
            let order_keys = std::iter::repeat_n(99_i64, 98)
                .chain([0_i64, 1_i64])
                .collect::<Vec<_>>();
            put_orders(&cassie, "budget_probe_orders", &order_keys);
            clear_cardinality(&cassie, "budget_probe_users");
            clear_cardinality(&cassie, "budget_probe_orders");

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    &select_inner_sql("budget_probe_users", "budget_probe_orders", 2),
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("user-000".to_string()), Value::Int64(0)],
                    vec![Value::String("user-001".to_string()), Value::Int64(1)],
                ]
            );
            assert_eq!(
                metrics["joins"]["last_bounded_side_selection_reason"].as_str(),
                Some("left_build_budget_exceeded")
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_not_apply_bounded_side_selection_to_left_join_or_ordered_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("bounded_selection_exclusions");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_join_tables(&cassie, &session, "exclude_users", "exclude_orders");
            put_users(&cassie, "exclude_users", 4);
            put_orders(&cassie, "exclude_orders", &[0, 1, 2, 3]);
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX exclude_orders_key_idx \
                 ON exclude_orders USING btree (order_user_key)",
                    vec![],
                )
                .unwrap();
            hydrate_cardinality(&cassie, "exclude_users");
            hydrate_cardinality(&cassie, "exclude_orders");
            let before = cassie.metrics();

            // Act
            let left_join = cassie
                .execute_sql(
                    &session,
                    "SELECT exclude_users.name, exclude_orders.total \
                 FROM exclude_users LEFT JOIN exclude_orders \
                 ON exclude_users.user_key = exclude_orders.order_user_key \
                 LIMIT 2",
                    vec![],
                )
                .unwrap();
            let ordered_join = cassie
                .execute_sql(
                    &session,
                    "SELECT exclude_users.name, exclude_orders.total \
                 FROM exclude_users JOIN exclude_orders \
                 ON exclude_users.user_key = exclude_orders.order_user_key \
                 ORDER BY exclude_users.name \
                 LIMIT 2",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(left_join.rows.len(), 2);
            assert_eq!(ordered_join.rows.len(), 2);
            assert_eq!(
                metric_delta(&after, &before, &["read_paths", "index_seek_scans"]),
                0
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/read_model_optimization_round.rs.
mod read_model_optimization_round {
    #![allow(unused_imports, dead_code)]

    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::Value;
    use tokio_postgres::NoTls;

    use super::support_sql as support;
    use support::*;

    fn vectorized_join_config() -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.vectorized_joins_enabled = true;
        config.limits.vectorized_join_batch_size = 2;
        config.limits.adaptive_execution_enabled = false;
        config.limits.operator_switching_enabled =
            cassie::config::OperatorSwitchingEnabled::disabled();
        config
    }

    fn canonical_collection(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    fn seed_read_model_hot_paths(cassie: &Cassie, session: &cassie::app::CassieSession) {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE read_model_hot_paths \
             (tenant TEXT, status TEXT, created_at INT, title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for (id, tenant, status, created_at, title, body) in [
            ("row-1", "tenant-a", "open", 30, "Gamma", "third"),
            ("row-2", "tenant-a", "open", 10, "Alpha", "first"),
            ("row-3", "tenant-a", "open", 20, "Beta", "second"),
            ("row-4", "tenant-a", "closed", 5, "Closed", "closed"),
            ("row-5", "tenant-b", "open", 1, "Other", "other"),
        ] {
            cassie
                .midge
                .put_document(
                    "read_model_hot_paths",
                    Some(id.to_string()),
                    serde_json::json!({
                        "tenant": tenant,
                        "status": status,
                        "created_at": created_at,
                        "title": title,
                        "body": body
                    }),
                )
                .unwrap();
        }
        for sql in [
            "CREATE INDEX read_model_hot_paths_tenant_status_time_idx \
         ON read_model_hot_paths USING btree (tenant, status, created_at)",
            "CREATE INDEX read_model_hot_paths_lower_title_idx \
         ON read_model_hot_paths USING btree (lower(title))",
        ] {
            cassie.execute_sql(session, sql, vec![]).unwrap();
        }
    }

    fn assert_hot_path_results(
        page: &cassie::executor::QueryResult,
        expression: &cassie::executor::QueryResult,
    ) {
        assert_eq!(
            page.rows,
            vec![
                vec![Value::String("first".to_string())],
                vec![Value::String("second".to_string())],
            ]
        );
        assert_eq!(
            expression.rows,
            vec![vec![Value::String("first".to_string())]]
        );
    }

    fn assert_hot_path_plans(
        page_explain: &cassie::executor::QueryResult,
        expression_explain: &cassie::executor::QueryResult,
    ) {
        let page_plan = explain_plan_text(page_explain);
        assert_explain_contains(
            page_plan,
            "index",
            "read_model_hot_paths_tenant_status_time_idx",
        );
        assert_explain_contains(page_plan, "access_path", "range_scan");
        assert_explain_contains(page_plan, "fallback_reason", "none");

        let expression_plan = explain_plan_text(expression_explain);
        assert_explain_contains(
            expression_plan,
            "index",
            "read_model_hot_paths_lower_title_idx",
        );
        assert_explain_contains(expression_plan, "access_path", "index_seek");
        assert_explain_contains(expression_plan, "fallback_reason", "none");
    }

    fn assert_hot_path_metrics(before: &serde_json::Value, after: &serde_json::Value) {
        assert!(
            after["read_paths"]["range_scans"].as_u64().unwrap()
                > before["read_paths"]["range_scans"].as_u64().unwrap()
        );
        assert!(
            after["read_paths"]["index_seek_scans"].as_u64().unwrap()
                > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
        );
    }

    fn seed_indexed_join_tables(cassie: &Cassie, session: &cassie::app::CassieSession) {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE read_model_indexed_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "CREATE TABLE read_model_indexed_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let users = (0..200)
            .map(|index| {
                (
                    Some(format!("user-{index:03}")),
                    serde_json::json!({
                        "user_key": i64::from(index),
                        "name": format!("user-{index:03}"),
                    }),
                )
            })
            .collect();
        let users_collection = canonical_collection("read_model_indexed_users");
        cassie
            .midge
            .put_fresh_documents(&users_collection, users)
            .unwrap();
        let orders_collection = canonical_collection("read_model_indexed_orders");
        cassie
            .midge
            .put_fresh_documents(
                &orders_collection,
                vec![
                    (
                        Some("order-150".to_string()),
                        serde_json::json!({"order_user_key": 150_i64, "total": 150_i64}),
                    ),
                    (
                        Some("order-151".to_string()),
                        serde_json::json!({"order_user_key": 151_i64, "total": 151_i64}),
                    ),
                ],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "CREATE INDEX read_model_indexed_users_key_idx \
             ON read_model_indexed_users USING btree (user_key)",
                vec![],
            )
            .unwrap();
    }

    fn assert_indexed_join_result(
        result: &cassie::executor::QueryResult,
        before: &serde_json::Value,
        after: &serde_json::Value,
    ) {
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-150".to_string()), Value::Int64(150)],
                vec![Value::String("user-151".to_string()), Value::Int64(151)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let probe_delta = after["joins"]["vectorized_probe_rows_total"]
            .as_u64()
            .unwrap()
            - before["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap();
        let index_seek_delta = after["read_paths"]["index_seek_scans"].as_u64().unwrap()
            - before["read_paths"]["index_seek_scans"].as_u64().unwrap();
        assert!(
            probe_delta <= 2,
            "expected indexed bounded join to probe only matching left rows, got {probe_delta}"
        );
        assert!(
            index_seek_delta > 0,
            "expected bounded inner join to use the indexed left source"
        );
    }

    #[test]
    fn should_lock_scalar_read_model_hot_path_access_paths() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_hot_paths");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            seed_read_model_hot_paths(&cassie, &session);
            let before = cassie.metrics();

            // Act
            let page = cassie
                .execute_sql(
                    &session,
                    "SELECT body FROM read_model_hot_paths \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let page_explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT body FROM read_model_hot_paths \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                    vec![],
                )
                .unwrap();
            let expression = cassie
                .execute_sql(
                    &session,
                    "SELECT body FROM read_model_hot_paths WHERE lower(title) = 'alpha'",
                    vec![],
                )
                .unwrap();
            let expression_explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT body FROM read_model_hot_paths WHERE lower(title) = 'alpha'",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_hot_path_results(&page, &expression);
            assert_hot_path_plans(&page_explain, &expression_explain);
            assert_hot_path_metrics(&before, &after);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_lock_column_batch_covered_read_hot_path() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_column_batch");
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
                    "CREATE TABLE read_model_column_batch \
                 (title TEXT, body TEXT, status TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            for (title, body, status, score) in [
                ("alpha", "one", "approved", 10),
                ("beta", "two", "pending", 20),
                ("gamma", "three", "approved", 30),
            ] {
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "INSERT INTO read_model_column_batch \
                         (title, body, status, score) VALUES \
                         ('{title}', '{body}', '{status}', {score})"
                        ),
                        vec![],
                    )
                    .unwrap();
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_model_column_batch_idx \
                 ON read_model_column_batch USING column (title, body, status, score) \
                 WITH (segment_size = 2)",
                    vec![],
                )
                .unwrap();
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title, body FROM read_model_column_batch \
                 WHERE status = 'approved' ORDER BY title",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title, body FROM read_model_column_batch \
                 WHERE status = 'approved' ORDER BY title",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![
                        Value::String("alpha".to_string()),
                        Value::String("one".to_string())
                    ],
                    vec![
                        Value::String("gamma".to_string()),
                        Value::String("three".to_string())
                    ],
                ]
            );
            let plan = explain_plan_text(&explain);
            assert_explain_contains(plan, "column_batch_index", "read_model_column_batch_idx");
            assert!(
                after["column_batches"]["scans"].as_u64().unwrap()
                    > before["column_batches"]["scans"].as_u64().unwrap()
            );
            assert!(
                after["column_batches"]["row_fetches_avoided"]
                    .as_u64()
                    .unwrap()
                    >= before["column_batches"]["row_fetches_avoided"]
                        .as_u64()
                        .unwrap()
                        + 2
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_lock_vectorized_join_hot_path_when_enabled() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_vectorized_join");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_model_join_users (user_key INT, name TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_model_join_orders (order_user_key INT, total INT)",
                    vec![],
                )
                .unwrap();
            for sql in [
                "INSERT INTO read_model_join_users (user_key, name) VALUES (1, 'ada')",
                "INSERT INTO read_model_join_users (user_key, name) VALUES (2, 'grace')",
                "INSERT INTO read_model_join_orders (order_user_key, total) VALUES (1, 42)",
                "INSERT INTO read_model_join_orders (order_user_key, total) VALUES (2, 7)",
            ] {
                cassie.execute_sql(&session, sql, vec![]).unwrap();
            }
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT read_model_join_users.name, read_model_join_orders.total \
                 FROM read_model_join_users JOIN read_model_join_orders \
                 ON read_model_join_users.user_key = read_model_join_orders.order_user_key \
                 ORDER BY read_model_join_users.name",
                    vec![],
                )
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT read_model_join_users.name, read_model_join_orders.total \
                 FROM read_model_join_users JOIN read_model_join_orders \
                 ON read_model_join_users.user_key = read_model_join_orders.order_user_key",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("ada".to_string()), Value::Int64(42)],
                    vec![Value::String("grace".to_string()), Value::Int64(7)],
                ]
            );
            let plan = explain_plan_text(&explain);
            assert_explain_contains(plan, "vectorized_join_candidate", "true");
            assert_explain_contains(plan, "vectorized_join_enabled", "true");
            assert_explain_contains(plan, "vectorized_join_fallback_reason", "none");
            assert!(
                after["joins"]["vectorized_joins"].as_u64().unwrap()
                    > before["joins"]["vectorized_joins"].as_u64().unwrap()
            );
            assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_stop_vectorized_join_after_unordered_limit_budget() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_vectorized_join_limit_budget");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = vectorized_join_config();
            config.limits.vectorized_join_batch_size = 8;
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_model_limit_users (user_key INT, name TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_model_limit_orders (order_user_key INT, total INT)",
                    vec![],
                )
                .unwrap();

            let mut users = Vec::new();
            let mut orders = Vec::new();
            for index in 0..64 {
                users.push((
                    Some(format!("user-{index:02}")),
                    serde_json::json!({
                        "user_key": i64::from(index),
                        "name": format!("user-{index:02}"),
                    }),
                ));
                orders.push((
                    Some(format!("order-{index:02}")),
                    serde_json::json!({
                        "order_user_key": i64::from(index),
                        "total": i64::from(index),
                    }),
                ));
            }
            cassie
                .midge
                .put_fresh_documents(&canonical_collection("read_model_limit_users"), users)
                .unwrap();
            cassie
                .midge
                .put_fresh_documents(&canonical_collection("read_model_limit_orders"), orders)
                .unwrap();
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT read_model_limit_users.name, read_model_limit_orders.total \
                 FROM read_model_limit_users JOIN read_model_limit_orders \
                 ON read_model_limit_users.user_key = read_model_limit_orders.order_user_key \
                 LIMIT 5",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 5);
            assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
            let output_delta = after["joins"]["output_rows_total"].as_u64().unwrap()
                - before["joins"]["output_rows_total"].as_u64().unwrap();
            let probe_delta = after["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap()
                - before["joins"]["vectorized_probe_rows_total"]
                    .as_u64()
                    .unwrap();
            assert_eq!(output_delta, 5);
            assert!(
                probe_delta <= 8,
                "expected limited join to probe at most one vectorized batch, got {probe_delta}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_push_unordered_left_join_limit_into_left_source_scan() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_left_join_source_budget");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = vectorized_join_config();
            config.limits.vectorized_join_batch_size = 8;
            config.limits.query_memory_budget_bytes = 4 * 1024;
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_model_budget_users (user_key INT, name TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_model_budget_orders (order_user_key INT, total INT)",
                    vec![],
                )
                .unwrap();

            let mut users = Vec::new();
            for index in 0..200 {
                users.push((
                    Some(format!("user-{index:03}")),
                    serde_json::json!({
                        "user_key": i64::from(index),
                        "name": format!("user-{index:03}"),
                    }),
                ));
            }
            cassie
                .midge
                .put_fresh_documents(&canonical_collection("read_model_budget_users"), users)
                .unwrap();
            cassie
                .midge
                .put_fresh_documents(
                    &canonical_collection("read_model_budget_orders"),
                    vec![(
                        Some("order-000".to_string()),
                        serde_json::json!({
                            "order_user_key": 0_i64,
                            "total": 42_i64,
                        }),
                    )],
                )
                .unwrap();
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT read_model_budget_users.name, read_model_budget_orders.total \
                 FROM read_model_budget_users LEFT JOIN read_model_budget_orders \
                 ON read_model_budget_users.user_key = read_model_budget_orders.order_user_key \
                 LIMIT 5",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 5);
            assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
            let probe_delta = after["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap()
                - before["joins"]["vectorized_probe_rows_total"]
                    .as_u64()
                    .unwrap();
            assert!(
            probe_delta <= 8,
            "expected limited left join to scan at most one left-source batch, got {probe_delta}"
        );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_probe_indexed_left_source_for_bounded_inner_join() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_indexed_inner_join_budget");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = vectorized_join_config();
            config.limits.vectorized_join_batch_size = 8;
            config.limits.query_memory_budget_bytes = 4 * 1024;
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_indexed_join_tables(&cassie, &session);
            let before = cassie.metrics();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT read_model_indexed_users.name, read_model_indexed_orders.total \
                 FROM read_model_indexed_users JOIN read_model_indexed_orders \
                 ON read_model_indexed_users.user_key = read_model_indexed_orders.order_user_key \
                 LIMIT 2",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();

            // Assert
            assert_indexed_join_result(&result, &before, &after);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_stream_unindexed_bounded_inner_join_until_output_budget() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_streaming_inner_join_budget");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        config.limits.query_memory_budget_bytes = 4 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_stream_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_stream_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_stream_users"), users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(
                &canonical_collection("read_model_stream_orders"),
                vec![
                    (
                        Some("order-000".to_string()),
                        serde_json::json!({
                            "order_user_key": 0_i64,
                            "total": 10_i64,
                        }),
                    ),
                    (
                        Some("order-001".to_string()),
                        serde_json::json!({
                            "order_user_key": 1_i64,
                            "total": 11_i64,
                        }),
                    ),
                ],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_stream_users.name, read_model_stream_orders.total \
                 FROM read_model_stream_users JOIN read_model_stream_orders \
                 ON read_model_stream_users.user_key = read_model_stream_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(10)],
                vec![Value::String("user-001".to_string()), Value::Int64(11)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let scanned_delta = after["read_paths"]["collection_scan_rows"]
            .as_u64()
            .unwrap()
            - before["read_paths"]["collection_scan_rows"]
                .as_u64()
                .unwrap();
        assert!(
            scanned_delta <= 10,
            "expected streaming bounded join to avoid full left materialization, scanned {scanned_delta} rows"
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_stream_dense_bounded_inner_join_without_materializing_right_source() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_dense_streaming_inner_join_budget");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        config.limits.query_memory_budget_bytes = 4 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_dense_stream_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_dense_stream_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        let mut orders = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
            orders.push((
                Some(format!("order-{index:03}")),
                serde_json::json!({
                    "order_user_key": 0_i64,
                    "total": i64::from(index),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_dense_stream_users"), users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_dense_stream_orders"), orders)
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_dense_stream_users.name, read_model_dense_stream_orders.total \
                 FROM read_model_dense_stream_users JOIN read_model_dense_stream_orders \
                 ON read_model_dense_stream_users.user_key = read_model_dense_stream_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(0)],
                vec![Value::String("user-000".to_string()), Value::Int64(1)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let scanned_delta = after["read_paths"]["collection_scan_rows"]
            .as_u64()
            .unwrap()
            - before["read_paths"]["collection_scan_rows"]
                .as_u64()
                .unwrap();
        assert!(
            scanned_delta <= 6,
            "expected dense bounded join to avoid materializing either source, scanned {scanned_delta} rows"
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_lock_pgwire_prepared_read_hot_path_metrics() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_model_pgwire_prepared");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE read_model_pgwire_prepared (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO read_model_pgwire_prepared (title, score) VALUES ('alpha', 7)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_model_pgwire_prepared_score_idx \
                 ON read_model_pgwire_prepared USING btree (score)",
                    vec![],
                )
                .unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                Arc::new(cassie.clone()),
                config,
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;

            let before = cassie.metrics();
            let mut client_config = tokio_postgres::Config::new();
            client_config.host("127.0.0.1");
            client_config.port(addr.port());
            client_config.user("root");
            client_config.password("postgres");
            client_config.dbname("postgres");
            let (client, connection) = client_config
                .connect(NoTls)
                .await
                .expect("connect tokio-postgres");
            let connection = tokio::spawn(async move {
                connection
                    .await
                    .expect("tokio-postgres connection should stay healthy");
            });

            // Act
            let statement = client
                .prepare("SELECT title FROM read_model_pgwire_prepared WHERE score = $1")
                .await
                .expect("prepare statement");
            let row = client
                .query_one(&statement, &[&7_i32])
                .await
                .expect("execute prepared statement");
            let title: String = row.try_get(0).expect("title value");
            drop(client);
            let _ = connection.await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            let after = cassie.metrics();

            // Assert
            assert_eq!(title, "alpha");
            assert!(
                after["pgwire"]["extended_queries_total"].as_u64().unwrap()
                    > before["pgwire"]["extended_queries_total"].as_u64().unwrap()
            );
            assert_eq!(
                after["pgwire"]["protocol_errors_total"].as_u64(),
                before["pgwire"]["protocol_errors_total"].as_u64()
            );
            assert!(
                after["read_paths"]["index_seek_scans"].as_u64().unwrap()
                    > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
            );

            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}
