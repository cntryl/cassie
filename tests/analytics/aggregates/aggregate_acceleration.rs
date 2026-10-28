#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::types::Value;

use super::support_sql as support;
use support::*;

#[test]
fn should_accelerate_numeric_aggregates_from_column_summaries() {
    // Arrange
    let path = data_dir("aggregate_accel_numeric");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        let sql = "SELECT COUNT(*) AS rows, COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_accel_numeric";
        let row_result = cassie
            .execute_sql(&session, sql, vec![])
            .expect("row-blob aggregate result");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_aggregate_accel_numeric ON aggregate_accel_numeric USING column (amount) WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(&session, sql, vec![])
            .expect("column-summary aggregate result");
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT COUNT(*) AS rows, SUM(amount) AS total FROM aggregate_accel_numeric",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows(), row_result.rows());
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            row_result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result.rows(),
            vec![vec![
                Value::Int64(3),
                Value::Int64(2),
                Value::Int64(10),
                Value::String("5.0000000000000000".to_owned()),
                Value::Int64(3),
                Value::Int64(7),
            ]]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
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
fn should_preserve_all_null_aggregate_results_in_column_summaries() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_accel_all_null (amount INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_accel_all_null (amount) VALUES (NULL), (NULL)",
                vec![],
            )
            .expect("insert NULL rows");
        let sql = "SELECT COUNT(*), COUNT(amount), SUM(amount), AVG(amount), MIN(amount), MAX(amount) FROM aggregate_accel_all_null";
        let row_result = cassie
            .execute_sql(&session, sql, vec![])
            .expect("row-blob aggregate result");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX aggregate_accel_all_null_idx ON aggregate_accel_all_null \
                 USING column (amount) WITH (segment_size = 1)",
                vec![],
            )
            .expect("create column index");
        let before = cassie.metrics();

        // Act
        let summary_result = cassie
            .execute_sql(&session, sql, vec![])
            .expect("column-summary aggregate result");
        let after = cassie.metrics();

        // Assert
        assert_eq!(summary_result.rows(), row_result.rows());
        assert_eq!(
            summary_result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            row_result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            summary_result.rows(),
            vec![vec![
                Value::Int64(2),
                Value::Int64(0),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ]]
        );
        assert!(
            after["aggregate_acceleration"]["scans"].as_u64().unwrap_or_default()
                > before["aggregate_acceleration"]["scans"].as_u64().unwrap_or_default(),
            "the all-NULL result should use column summaries"
        );
    });
}

#[test]
fn should_preserve_aggregate_results_across_column_segment_boundaries() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let outcomes = runtime.block_on(async {
        let mut outcomes = Vec::new();
        for segment_size in [1, 2, 3] {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie.execute_sql(&session, "CREATE TABLE segment_aggregate (amount INT)", vec![])
                .expect("create table");
            cassie.execute_sql(&session, "INSERT INTO segment_aggregate VALUES (7), (NULL), (3)", vec![])
                .expect("insert rows");
            let sql = "SELECT COUNT(*), COUNT(amount), SUM(amount), AVG(amount), MIN(amount), MAX(amount) FROM segment_aggregate";
            let rows = cassie.execute_sql(&session, sql, vec![]).expect("row result");
            cassie.execute_sql(&session, &format!("CREATE INDEX segment_aggregate_idx ON segment_aggregate USING column (amount) WITH (segment_size = {segment_size})"), vec![])
                .expect("create column index");
            let before = cassie.metrics()["aggregate_acceleration"]["scans"].as_u64().unwrap_or_default();

            // Act
            let summary = cassie.execute_sql(&session, sql, vec![]).expect("summary result");
            let after = cassie.metrics()["aggregate_acceleration"]["scans"].as_u64().unwrap_or_default();
            outcomes.push((segment_size, rows, summary, before, after));
        }
        outcomes
    });

    // Assert
    for (segment_size, rows, summary, before, after) in outcomes {
        assert_eq!(summary.rows(), rows.rows(), "segment size {segment_size}");
        assert_eq!(
            summary.rows(),
            vec![vec![
                Value::Int64(3),
                Value::Int64(2),
                Value::Int64(10),
                Value::String("5.0000000000000000".to_owned()),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );
        assert_eq!(
            summary
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            rows.columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            "segment size {segment_size}"
        );
        assert_eq!(
            summary
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            [20, 20, 20, 1700, 23, 23]
        );
        assert!(
            after > before,
            "summary path was not used for segment size {segment_size}"
        );
    }
}

#[test]
fn should_accelerate_aggregates_for_case_distinct_columns_independently() {
    // Arrange
    let path = data_dir("aggregate_accel_case_distinct_columns");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_case_fields (\"a\" INT, \"A\" INT)",
                vec![],
            )
            .expect("create case-distinct table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_case_fields (\"a\", \"A\") VALUES (1, 10)",
                vec![],
            )
            .expect("insert first case-distinct row");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_case_fields (\"a\", \"A\") VALUES (2, 20)",
                vec![],
            )
            .expect("insert second case-distinct row");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX aggregate_case_fields_idx ON aggregate_case_fields \
                 USING column (\"a\", \"A\")",
                vec![],
            )
            .expect("create column index");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT SUM(\"a\"), SUM(\"A\") FROM aggregate_case_fields",
                vec![],
            )
            .expect("aggregate exact case-distinct fields");

        // Assert
        assert_eq!(result.rows(), vec![vec![Value::Int64(3), Value::Int64(30)]]);
        assert_eq!(cassie.metrics()["aggregate_acceleration"]["scans"], 1);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_maintain_aggregate_summaries_after_update_delete() {
    // Arrange
    let path = data_dir("aggregate_accel_maintenance");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
            result.rows(),
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
    let path = data_dir("aggregate_accel_group_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
            result.rows(),
            vec![
                vec![Value::String("a".to_string()), Value::Int64(10)],
                vec![Value::String("b".to_string()), Value::Int64(5)],
            ]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("aggregate_acceleration=false"));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_to_rows_given_aggregate_local_modifiers_when_column_summaries_are_eligible() {
    // Arrange
    let path = data_dir("aggregate_accel_local_modifiers");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_accel_local_modifiers (included BOOLEAN, amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_accel_local_modifiers VALUES (true, 2)",
            "INSERT INTO aggregate_accel_local_modifiers VALUES (true, 2)",
            "INSERT INTO aggregate_accel_local_modifiers VALUES (true, 3)",
            "INSERT INTO aggregate_accel_local_modifiers VALUES (false, 9)",
            "INSERT INTO aggregate_accel_local_modifiers VALUES (true, NULL)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_aggregate_accel_local_modifiers ON aggregate_accel_local_modifiers USING column (included, amount) WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();

        // Act
        let baseline = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT SUM(amount) FROM aggregate_accel_local_modifiers",
                vec![],
            )
            .unwrap();
        let sql = "SELECT SUM(amount), SUM(DISTINCT amount ORDER BY amount DESC) FILTER (WHERE included) FROM aggregate_accel_local_modifiers";
        let result = cassie.execute_sql(&session, sql, vec![]).unwrap();
        let explain = cassie
            .execute_sql(&session, &format!("EXPLAIN {sql}"), vec![])
            .unwrap();

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![Value::Int64(16), Value::Int64(5)]]
        );
        let Value::String(baseline_plan) = &baseline.rows()[0][0] else {
            panic!("expected textual baseline plan");
        };
        assert!(baseline_plan.contains("aggregate_acceleration=true"));
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("aggregate_acceleration=false"));
    });

    let _ = std::fs::remove_dir_all(path);
}
