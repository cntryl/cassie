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
    let path = data_dir("analytical_projection_options");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
    let path = data_dir("analytical_projection_route");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
            result.rows(),
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
        let Value::String(plan) = &explained.rows()[0][0] else {
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
fn should_match_staged_transaction_rows_in_analytical_projection_reads() {
    // Arrange
    let path = data_dir("analytical_projection_transaction_overlay");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE ap_tx_source (tenant TEXT, amount INT)",
                "INSERT INTO ap_tx_source VALUES ('acme', 10), ('acme', 11)",
                "CREATE MATERIALIZED PROJECTION ap_tx WITH (analytical = true) AS SELECT tenant, amount FROM ap_tx_source",
                "BEGIN",
                "INSERT INTO ap_tx_source VALUES ('acme', 20)",
            ] {
                cassie.execute_sql(&session, sql, Vec::new()).expect("setup SQL");
            }

            // Act
            let after_insert = cassie
                .execute_sql(
                    &session,
                    "SELECT tenant, amount FROM ap_tx_source ORDER BY amount",
                    Vec::new(),
                )
                .expect("read after staged insert");
            cassie
                .execute_sql(&session, "DELETE FROM ap_tx_source WHERE amount = 11", Vec::new())
                .expect("stage delete");
            let after_delete = cassie
                .execute_sql(
                    &session,
                    "SELECT tenant, amount FROM ap_tx_source ORDER BY amount",
                    Vec::new(),
                )
                .expect("read after staged delete");

            // Assert
            assert_eq!(
                after_insert.rows(),
                vec![
                    vec![Value::String("acme".to_string()), Value::Int64(10)],
                    vec![Value::String("acme".to_string()), Value::Int64(11)],
                    vec![Value::String("acme".to_string()), Value::Int64(20)],
                ]
            );
            assert_eq!(
                after_delete.rows(),
                vec![
                    vec![Value::String("acme".to_string()), Value::Int64(10)],
                    vec![Value::String("acme".to_string()), Value::Int64(20)],
                ]
            );
            cassie
                .execute_sql(&session, "ROLLBACK", Vec::new())
                .expect("rollback transaction");
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_fallback_to_source_when_analytical_projection_is_stale() {
    // Arrange
    let path = data_dir("analytical_projection_stale_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
            result.rows(),
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
    let path = data_dir("analytical_projection_output_dml");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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

#[test]
fn should_read_a_capitalized_projection_alias_in_lowercase() {
    // Arrange
    let path = data_dir("projection_alias_case");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE alias_src (amount INT, tag TEXT)",
        "INSERT INTO alias_src (amount, tag) VALUES (5, 'a'), (7, 'b')",
        "CREATE MATERIALIZED PROJECTION alias_p AS SELECT amount AS Total, tag FROM alias_src",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }
    let read = |sql: &str| {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect(sql)
            .rows()
            .to_vec()
    };

    // Act
    let projected = read("SELECT total, tag FROM alias_p ORDER BY tag");
    let filtered = read("SELECT tag FROM alias_p WHERE total = 5");
    let summed = read("SELECT SUM(total) FROM alias_p");
    let top = read("SELECT total FROM alias_p ORDER BY total DESC LIMIT 1");

    // Assert
    assert_eq!(
        projected,
        vec![
            vec![Value::Int64(5), Value::String("a".to_string())],
            vec![Value::Int64(7), Value::String("b".to_string())]
        ]
    );
    assert_eq!(filtered, vec![vec![Value::String("a".to_string())]]);
    assert_eq!(summed, vec![vec![Value::Int64(12)]]);
    assert_eq!(top, vec![vec![Value::Int64(7)]]);
    let _ = std::fs::remove_dir_all(path);
}
