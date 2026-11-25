#[path = "../../../benches/support/column_diagnostics.rs"]
mod column_diagnostics;

use cassie::types::typed::TypedValue;
use cassie::types::Value;

#[test]
fn should_return_column_aggregate_values_when_types_match() {
    // Arrange
    let row = vec![
        Value::Int64(5),
        Value::Int64(470_000),
        Value::String("94.0000000000000000".to_string()),
        Value::Int64(90),
        Value::Int64(98),
    ];

    // Act
    let values = column_diagnostics::column_aggregate_result_or_panic(&row);

    // Assert
    assert_eq!(*values.row_count, 5);
    assert_eq!(*values.score_sum, 470_000);
    assert_eq!(values.score_average, "94.0000000000000000");
    assert_eq!(*values.min_score, 90);
    assert_eq!(*values.max_score, 98);
}

#[test]
fn should_panic_with_redacted_types_for_unexpected_column_aggregate_values() {
    // Arrange
    let row = vec![
        Value::String("test-secret".to_string()),
        Value::Typed(TypedValue::Macaddr([0xaa; 6])),
        Value::Typed(TypedValue::Macaddr8([0xbb; 8])),
    ];

    // Act
    let Err(panic) =
        std::panic::catch_unwind(|| column_diagnostics::column_aggregate_result_or_panic(&row))
    else {
        panic!("unexpected aggregate types must panic");
    };
    let message = panic
        .downcast_ref::<String>()
        .expect("panic diagnostic should be a string");

    // Assert
    assert_eq!(
        message,
        "Tier 3 column aggregate result has unexpected types: [string, typed, typed]"
    );
    assert!(message.contains("[string, typed, typed]"));
    assert!(!message.contains("test-secret"));
    assert!(!message.contains("170"));
    assert!(!message.contains("187"));
}
