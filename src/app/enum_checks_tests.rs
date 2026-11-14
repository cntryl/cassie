use super::super::Cassie;
use crate::catalog::{CollectionSchema, ConstraintCheck, ConstraintOperator, FieldMeta};
use crate::types::enum_type::{EnumMember, EnumType, ENUM_STORAGE_KEY};
use crate::types::{DataType, EnumRef};
use serde_json::json;

/// `level` is ('a', 'b', 'c') by sort order while the member ids run the other
/// way, as after `ADD VALUE ... BEFORE`.
fn schema() -> CollectionSchema {
    let member = |member_id, label: &str, sort_order| EnumMember {
        member_id,
        label: label.to_owned(),
        sort_order,
        uncommitted: false,
    };
    let level = DataType::Enum(EnumRef::new(EnumType {
        oid: 40_001,
        array_oid: 40_002,
        qualified_name: "db.public.level".to_owned(),
        name: "level".to_owned(),
        revision: 2,
        members: vec![
            member(30, "a", 1.0),
            member(20, "b", 2.0),
            member(10, "c", 3.0),
        ],
    }));
    let mut schema = CollectionSchema::new("db.public.items");
    schema
        .fields
        .push(FieldMeta::declared("s".to_owned(), level, false, None));
    schema
}

fn stored(member_id: u64) -> serde_json::Value {
    json!({ ENUM_STORAGE_KEY: member_id })
}

fn check(operator: ConstraintOperator, member_id: u64) -> ConstraintCheck {
    ConstraintCheck {
        field: "s".to_owned(),
        operator,
        value: stored(member_id),
        bound_expression: None,
    }
}

#[test]
fn should_compare_enum_check_members_by_sort_order() {
    // Arrange
    let schema = schema();
    let below_c = check(ConstraintOperator::Lt, 10);
    let a = stored(30);
    let c = stored(10);

    // Act
    let a_below_c = Cassie::satisfies_column_check(Some(&schema), Some(&a), &below_c);
    let c_below_c = Cassie::satisfies_column_check(Some(&schema), Some(&c), &below_c);

    // Assert
    assert!(matches!(a_below_c, Ok(true)));
    assert!(matches!(c_below_c, Ok(false)));
}

#[test]
fn should_evaluate_enum_check_equality_on_members() {
    // Arrange
    let schema = schema();
    let not_c = check(ConstraintOperator::NotEq, 10);
    let b = stored(20);
    let c = stored(10);

    // Act
    let b_allowed = Cassie::satisfies_column_check(Some(&schema), Some(&b), &not_c);
    let c_allowed = Cassie::satisfies_column_check(Some(&schema), Some(&c), &not_c);

    // Assert
    assert!(matches!(b_allowed, Ok(true)));
    assert!(matches!(c_allowed, Ok(false)));
}

#[test]
fn should_satisfy_an_enum_check_for_a_null_value() {
    // Arrange
    let schema = schema();
    let equals_a = check(ConstraintOperator::Eq, 30);

    // Act
    let satisfied = Cassie::satisfies_column_check(Some(&schema), None, &equals_a);

    // Assert
    assert!(matches!(satisfied, Ok(true)));
}
