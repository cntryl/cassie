use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[test]
fn should_accelerate_filtered_numeric_aggregates_over_encoded_selection() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
