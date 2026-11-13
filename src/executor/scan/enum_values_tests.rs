use super::*;
use crate::types::enum_type::{sort_order_at, ENUM_STORAGE_KEY};
use crate::types::{EnumMember, EnumType};

fn mood() -> EnumRef {
    EnumRef::new(EnumType {
        oid: 300_001,
        array_oid: 300_002,
        qualified_name: "db.public.mood".to_string(),
        name: "mood".to_string(),
        revision: 2,
        members: ["sad", "ok"]
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
fn should_restore_a_stored_member_id_with_the_current_label() {
    // Arrange
    let data_type = DataType::Enum(mood());
    let stored = serde_json::json!({ ENUM_STORAGE_KEY: 7_001 });

    // Act
    let value = from_storage(&stored, &data_type).expect("restore enum");

    // Assert
    let Value::Typed(TypedValue::Enum(restored)) = value else {
        panic!("expected an enum value");
    };
    assert_eq!(restored.label(), "ok");
}

#[test]
fn should_report_corruption_for_an_unknown_member_id() {
    // Arrange
    let data_type = DataType::Enum(mood());
    let stored = serde_json::json!({ ENUM_STORAGE_KEY: 9_999 });

    // Act
    let error = from_storage(&stored, &data_type).expect_err("unknown member");

    // Assert
    assert_eq!(error.sql_state(), "XX001");
}

#[test]
fn should_restore_a_stored_enum_array_with_typed_elements() {
    // Arrange
    let data_type = DataType::Array(Box::new(DataType::Enum(mood())));
    let array = SqlArray::new(
        vec![crate::types::ArrayDimension {
            length: 2,
            lower_bound: 1,
        }],
        vec![
            Value::Json(serde_json::json!({ ENUM_STORAGE_KEY: 7_000 })),
            Value::Null,
        ],
    )
    .expect("array");

    // Act
    let value = from_storage(&array.to_storage_json(), &data_type).expect("restore array");

    // Assert
    let Value::Array(restored) = value else {
        panic!("expected an array");
    };
    assert!(matches!(
        &restored.elements()[0],
        Value::Typed(TypedValue::Enum(first)) if first.label() == "sad"
    ));
    assert_eq!(restored.elements()[1], Value::Null);
}

#[test]
fn should_keep_sql_null_for_a_stored_json_null() {
    // Arrange
    let data_type = DataType::Enum(mood());

    // Act
    let value = from_storage(&serde_json::Value::Null, &data_type).expect("restore NULL");

    // Assert
    assert_eq!(value, Value::Null);
}
