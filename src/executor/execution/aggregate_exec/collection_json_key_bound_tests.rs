//! SQL array object keys retain bounds that JSON array values omit.
use super::*;
use crate::types::{ArrayDimension, SqlArray};

#[test]
fn should_admit_nondefault_array_bound_prefixes_before_json_object_rendering() {
    // Arrange
    let key = Value::Array(
        SqlArray::new(
            vec![
                ArrayDimension {
                    length: 1,
                    lower_bound: i32::MIN
                };
                6
            ],
            vec![Value::String(String::new())],
        )
        .expect("rank-six bounded key"),
    );
    let values = vec![(key, Some(Value::Json(serde_json::Value::Null)))];
    let context = ConversionContext {
        value_type: None,
        controls: None,
    };
    let kind = CollectionAggregate::JsonObject {
        binary: true,
        strict: false,
        unique: false,
    };

    // Act
    let bound = output_bound(&values, &context, true, true).expect("key and document bound");
    let output = finish_unscoped(kind, values, true).expect("array key object");

    // Assert
    let Value::Json(document) = output else {
        panic!("native object document")
    };
    let text = crate::types::jsonb::format(&document).expect("canonical object text");
    assert!(
        bound >= text.len(),
        "SQL bound prefixes must be admitted before rendering: {bound} < {}",
        text.len()
    );
    assert_eq!(document.as_object().expect("object").len(), 1);
}
