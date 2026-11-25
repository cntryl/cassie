use super::support_array_order::seed_numeric_arrays;
use super::support_read_equivalence::{sql, with_fixture};
use cassie::types::Value;

#[test]
fn should_select_elementwise_extrema_for_declared_arrays() {
    // Arrange
    with_fixture("typed_array_extrema", |cassie, session| {
        seed_numeric_arrays(cassie, session);
        // Act
        let json_errors = ["MIN", "MAX"].map(|aggregate| {
            cassie
                .execute_sql(
                    session,
                    &format!("SELECT {aggregate}(arr) FROM json_values"),
                    vec![],
                )
                .expect_err("PostgreSQL has no JSON extrema overload")
                .to_string()
        });
        let arrays = sql(
            cassie,
            session,
            "SELECT MIN(arr),MAX(arr) FROM array_values",
        );
        // Assert
        assert_eq!(
            json_errors,
            [
                "execution error: function min(json) does not exist",
                "execution error: function max(json) does not exist",
            ]
        );
        assert_eq!(
            arrays.rows(),
            vec![vec![
                Value::Array(cassie::types::SqlArray::empty()),
                Value::Array(
                    cassie::types::SqlArray::from_elements(vec![Value::Int64(10)])
                        .expect("one-dimensional array")
                )
            ]]
        );
    });
}
#[test]
fn should_merge_partial_array_extrema_without_losing_element_order() {
    // Arrange
    super::support_read_equivalence::with_fixture_config(
        "typed_array_partial_extrema",
        |config| {
            config.limits.parallel_aggregation_workers = 4;
            config.limits.query_memory_budget_bytes = 16 * 1024 * 1024;
        },
        |cassie, session| {
            sql(
                cassie,
                session,
                "CREATE TABLE partial_arrays (arr BIGINT[])",
            );
            let documents = (0..1025)
                .map(|index| {
                    let array = match index {
                        0 => serde_json::json!([2]),
                        1024 => serde_json::json!([10]),
                        _ => serde_json::json!([3]),
                    };
                    (
                        Some(format!("{index:04}")),
                        serde_json::json!({"arr":array}),
                    )
                })
                .collect();
            cassie
                .midge
                .put_documents("partial_arrays", documents)
                .expect("partial ARRAY fixture");
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
                    Value::Array(
                        cassie::types::SqlArray::from_elements(vec![Value::Int64(2)])
                            .expect("one-dimensional array")
                    ),
                    Value::Array(
                        cassie::types::SqlArray::from_elements(vec![Value::Int64(10)])
                            .expect("one-dimensional array")
                    )
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
