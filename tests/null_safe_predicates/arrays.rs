use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_array_order_length_null_slots_at_null_safe_equality() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_array_values",
        &["CREATE TABLE array_pairs (id INT, l BIGINT[], r BIGINT[])"],
    );
    let cases = [
        (
            Value::Json(serde_json::json!([1, null, 2])),
            Value::Json(serde_json::json!([1, null, 2])),
            false,
        ),
        (
            Value::Json(serde_json::json!([1, null, 2])),
            Value::Json(serde_json::json!([2, null, 1])),
            true,
        ),
        (
            Value::Json(serde_json::json!([1, null, 2])),
            Value::Json(serde_json::json!([1, null, 2, 2])),
            true,
        ),
        (Value::Json(serde_json::json!([])), Value::Null, true),
        (Value::Null, Value::Null, false),
        (
            Value::Json(serde_json::json!([])),
            Value::Json(serde_json::json!([])),
            false,
        ),
    ];
    for (index, (left, right, _)) in cases.iter().enumerate() {
        fixture
            .cassie
            .execute_sql(
                &fixture.session,
                &format!("INSERT INTO array_pairs VALUES ({index}, $1, $2)"),
                vec![left.clone(), right.clone()],
            )
            .expect("declared ARRAY input");
    }
    // Act
    let rows = fixture.rows("SELECT l IS DISTINCT FROM r AS different, l IS NOT DISTINCT FROM r AS same FROM array_pairs ORDER BY id");
    // Assert
    let expected = cases
        .iter()
        .map(|(_, _, distinct)| vec![Value::Bool(*distinct), Value::Bool(!distinct)])
        .collect::<Vec<_>>();
    assert_eq!(rows, expected);
}
