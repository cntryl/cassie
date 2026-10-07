//! Per-codec witnesses run the real common scan and typed aggregate worker path.
use super::*;
use crate::config::CassieRuntimeLimits;
use std::time::Instant;

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) -> Result<Vec<Vec<Value>>, String> {
    let statement = crate::sql::parse_statement(sql).expect("statement");
    let plan =
        super::super::super::build_logical_plan_in_session(cassie, Some(session), &statement)
            .expect("plan");
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let result = try_execute(cassie, Some(session), &plan, &controls)
        .map(|output| {
            let output = output.expect("selected typed path");
            output
                .iter()
                .map(|row| {
                    row.entries()
                        .iter()
                        .map(|(_, value)| value.clone())
                        .collect()
                })
                .collect()
        })
        .map_err(|error| error.to_string());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    result
}
fn cases() -> Vec<(&'static str, &'static str, Vec<serde_json::Value>)> {
    vec![
        ("constant", "BIGINT", vec![serde_json::json!(7); 2048]),
        (
            "frame_of_reference",
            "BIGINT",
            (0..2048)
                .map(|row| serde_json::json!(9_007_199_254_740_993_i64 + i64::from(row)))
                .collect(),
        ),
        (
            "rle",
            "BIGINT",
            (0..2048)
                .map(|row| serde_json::json!(if row < 1024 { i64::MIN } else { i64::MAX }))
                .collect(),
        ),
        (
            "dictionary",
            "BIGINT",
            (0..2048)
                .map(|row| serde_json::json!([i64::MIN, 0, i64::MAX][row % 3]))
                .collect(),
        ),
        (
            "plain",
            "BIGINT",
            (0_u64..2048)
                .map(|row| {
                    let bits = row.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17);
                    serde_json::json!(i64::from_ne_bytes(bits.to_ne_bytes()))
                })
                .collect(),
        ),
        (
            "alp",
            "FLOAT",
            (0..2048)
                .map(|row| {
                    if row % 17 == 0 {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!((f64::from(row) - 1024.0) / 4.0)
                    }
                })
                .collect(),
        ),
        (
            "rle",
            "FLOAT",
            (0..2048)
                .map(|row| serde_json::json!(if row < 1024 { f64::MAX } else { f64::MIN }))
                .collect(),
        ),
        (
            "plain",
            "FLOAT",
            (0..2048)
                .map(|row| serde_json::json!([f64::MIN, 0.0, f64::MAX][row % 3]))
                .collect(),
        ),
    ]
}

#[test]
fn should_match_row_aggregate_semantics_through_every_selected_numeric_codec() {
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    for (expected_codec, data_type, values) in cases() {
        let path =
            std::env::temp_dir().join(format!("cassie-typed-codec-{}", uuid::Uuid::new_v4()));
        let mut config = crate::config::CassieRuntimeConfig::from_env().expect("config");
        config.limits.parallel_aggregation_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config)
            .expect("Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE codec (n {data_type})"),
                vec![],
            )
            .expect("table");
        cassie
            .midge
            .put_fresh_documents(
                "codec",
                values
                    .iter()
                    .cycle()
                    .take(4096)
                    .enumerate()
                    .map(|(row, value)| (Some(format!("{row:08}")), serde_json::json!({"n":value})))
                    .collect(),
            )
            .expect("seed");
        let queries = [
            "SELECT COUNT(*),COUNT(n),MIN(n),MAX(n) FROM codec",
            "SELECT SUM(n) FROM codec",
            "SELECT AVG(n) FROM codec",
        ];
        let expected = queries
            .iter()
            .map(|sql| execute(&cassie, &session, sql))
            .collect::<Vec<_>>();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX codec_idx ON codec USING column (n) WITH (segment_size = 2048)",
                vec![],
            )
            .expect("index");
        let metadata = cassie
            .midge
            .get_column_batch_metadata("codec", "codec_idx")
            .expect("metadata read")
            .expect("metadata");
        let before = cassie.runtime.snapshot();
        // Act
        let actual = queries
            .iter()
            .map(|sql| execute(&cassie, &session, sql))
            .collect::<Vec<_>>();
        let after = cassie.runtime.snapshot();
        // Assert
        assert!(
            metadata
                .segments
                .iter()
                .all(|segment| segment.field_chunks["n"].codec_name == expected_codec),
            "{data_type} {expected_codec}"
        );
        assert_eq!(actual, expected, "{data_type} {expected_codec}");
        assert_eq!(
            after.column_batches.materialized_values, before.column_batches.materialized_values,
            "{expected_codec}"
        );
        assert!(after.column_batches.scans > before.column_batches.scans);
        assert!(
            after.parallel_aggregation.aggregations > before.parallel_aggregation.aggregations,
            "COUNT/extrema merge must execute two encoded segments"
        );
        drop((metadata, session, cassie));
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_decline_native_aggregation_before_opening_inputs_when_the_transport_floor_is_unavailable()
{
    // Arrange
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let path = std::env::temp_dir().join(format!("cassie-typed-floor-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE floor (n BIGINT)", vec![])
        .expect("table");
    let statement = crate::sql::parse_statement("SELECT SUM(n) FROM floor").expect("statement");
    let plan =
        super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
            .expect("plan");
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let before = cassie.runtime.snapshot();
    // Act
    let declined =
        try_execute(&cassie, Some(&session), &plan, &controls).expect("controlled floor decline");
    let after = cassie.runtime.snapshot();
    // Assert
    assert!(declined.is_none());
    assert_eq!(after.storage.data.reads, before.storage.data.reads);
    assert_eq!(
        after.parallel_aggregation.aggregations,
        before.parallel_aggregation.aggregations
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}
