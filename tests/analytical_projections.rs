#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::sql::ast::QueryStatement;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
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
    with_fallback();
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
    with_fallback();
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
        assert!(plan.contains("analytical_projection=analytical_route"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fallback_to_source_when_analytical_projection_is_stale() {
    // Arrange
    with_fallback();
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
    with_fallback();
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

        // Act
        let error = cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO {output} (tenant, amount) VALUES ('acme', 10)"),
                vec![],
            )
            .unwrap_err();

        // Assert
        assert!(error.to_string().contains("read-only"));

        let _ = std::fs::remove_dir_all(path);
    });
}
