#[path = "../../../benches/support/column_diagnostics.rs"]
mod column_diagnostics;

use cassie::types::typed::TypedValue;
use cassie::types::Value;

#[test]
fn should_report_column_result_types_without_logging_values() {
    // Arrange
    let row = vec![
        Value::String("test-secret".to_string()),
        Value::Typed(TypedValue::Macaddr([0xaa; 6])),
        Value::Typed(TypedValue::Macaddr8([0xbb; 8])),
    ];

    // Act
    let message = column_diagnostics::column_aggregate_unexpected_types(&row);

    // Assert
    assert_eq!(
        message,
        "Tier 3 column aggregate result has unexpected types: [string, typed, typed]"
    );
    assert!(!message.contains("test-secret"));
    assert!(!message.contains("170"));
    assert!(!message.contains("187"));
}
