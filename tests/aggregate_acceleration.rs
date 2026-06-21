#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_accelerate_numeric_aggregates_from_column_summaries() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
