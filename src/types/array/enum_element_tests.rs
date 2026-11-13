use super::{format_text_array, parse_text_array};
use crate::types::enum_type::{sort_order_at, ENUM_STORAGE_KEY};
use crate::types::{DataType, EnumMember, EnumRef, EnumType, SqlArray, Value};

fn mood() -> EnumRef {
    EnumRef::new(EnumType {
        oid: 300_001,
        array_oid: 300_002,
        qualified_name: "db.public.mood".to_string(),
        name: "mood".to_string(),
        revision: 1,
        members: ["sad", "ok", "happy"]
            .iter()
            .enumerate()
            .map(|(position, label)| EnumMember {
                member_id: 7_000 + u64::try_from(position).expect("small position"),
                label: (*label).to_string(),
                sort_order: sort_order_at(position),
                uncommitted: false,
            })
            .collect(),
    })
}

#[test]
fn should_parse_text_array_labels_to_the_stored_scalar_form() {
    // Arrange
    let element_type = DataType::Enum(mood());

    // Act
    let values = parse_text_array("{sad,happy}", &element_type).expect("parse enum array");

    // Assert
    assert_eq!(
        values,
        serde_json::json!([{ ENUM_STORAGE_KEY: 7_000 }, { ENUM_STORAGE_KEY: 7_002 }])
    );
}

#[test]
fn should_reject_an_unknown_label_in_an_enum_array_literal() {
    // Arrange
    let element_type = DataType::Enum(mood());

    // Act
    let result = parse_text_array("{sad,furious}", &element_type);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_build_a_sql_array_of_enum_documents_from_labels_and_documents() {
    // Arrange
    let element_type = DataType::Enum(mood());
    let source = serde_json::json!(["ok", { ENUM_STORAGE_KEY: 7_002 }, null]);

    // Act
    let array = SqlArray::from_nested_json_typed(&source, &element_type).expect("enum array");

    // Assert
    assert_eq!(
        array.elements(),
        &[
            Value::Json(serde_json::json!({ ENUM_STORAGE_KEY: 7_001 })),
            Value::Json(serde_json::json!({ ENUM_STORAGE_KEY: 7_002 })),
            Value::Null,
        ]
    );
}

#[test]
fn should_format_enum_array_elements_as_labels() {
    // Arrange
    let element_type = DataType::Enum(mood());
    let stored =
        serde_json::json!([{ ENUM_STORAGE_KEY: 7_002 }, null, { ENUM_STORAGE_KEY: 7_000 }]);

    // Act
    let text = format_text_array(&stored, &element_type);

    // Assert
    assert_eq!(text.as_deref(), Some("{happy,NULL,sad}"));
}
