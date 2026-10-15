use super::support_array_order::seed_numeric_arrays;
use super::support_read_equivalence::{sql, with_fixture};
use cassie::types::Value;

#[test]
fn should_select_elementwise_extrema_for_declared_arrays() {
    // Arrange
    with_fixture("typed_array_extrema", |cassie, session| {
        seed_numeric_arrays(cassie, session);
        let json = sql(cassie, session, "SELECT MIN(arr),MAX(arr) FROM json_values");
        assert_eq!(
            json.rows(),
            vec![vec![
                Value::Json(serde_json::json!([1, 9])),
                Value::Json(serde_json::json!([]))
            ]]
        );
        // Act
        let arrays = sql(
            cassie,
            session,
            "SELECT MIN(arr),MAX(arr) FROM array_values",
        );
        // Assert
        assert_eq!(
            arrays.rows(),
            vec![vec![
                Value::Json(serde_json::json!([])),
                Value::Json(serde_json::json!([10]))
            ]]
        );
    });
}
#[test]
fn should_merge_partial_array_extrema_without_losing_element_order() {
    // Arrange
    super::support_read_equivalence::with_fixture_config(
        "typed_array_partial_extrema",
        |config| config.limits.parallel_aggregation_workers = 4,
        |cassie, session| {
            sql(
                cassie,
                session,
                "CREATE TABLE partial_arrays (arr BIGINT[])",
            );
            for index in 0..1025 {
                let array = match index {
                    0 => serde_json::json!([2]),
                    1024 => serde_json::json!([10]),
                    _ => serde_json::json!([3]),
                };
                cassie
                    .midge
                    .put_document(
                        "partial_arrays",
                        Some(format!("{index:04}")),
                        serde_json::json!({"arr":array}),
                    )
                    .expect("partial ARRAY fixture");
            }
            // Act
            let result = sql(
                cassie,
                session,
                "SELECT MIN(arr), MAX(arr) FROM partial_arrays",
            );
            let metrics = cassie.metrics();
            // Assert
            assert_eq!(
                result.rows(),
                vec![vec![
                    Value::Json(serde_json::json!([2])),
                    Value::Json(serde_json::json!([10]))
                ]]
            );
            assert!(
                metrics["parallel_aggregation"]["workers"]
                    .as_u64()
                    .unwrap_or(0)
                    >= 2
            );
        },
    );
}
