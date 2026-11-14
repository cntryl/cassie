use super::bind;
use crate::catalog::{ConstraintCheck, ConstraintOperator, FieldConstraint};
use crate::types::enum_type::{EnumMember, EnumType};
use crate::types::{DataType, EnumRef, FieldSchema};
use serde_json::json;

fn level_fields() -> Vec<FieldSchema> {
    let member = |member_id, label: &str, sort_order| EnumMember {
        member_id,
        label: label.to_owned(),
        sort_order,
        uncommitted: false,
    };
    vec![FieldSchema {
        name: "s".to_owned(),
        data_type: DataType::Enum(EnumRef::new(EnumType {
            oid: 30_001,
            array_oid: 30_002,
            qualified_name: "db.public.level".to_owned(),
            name: "level".to_owned(),
            revision: 1,
            members: vec![member(11, "a", 1.0), member(12, "b", 2.0)],
        })),
        nullable: true,
    }]
}

fn check(operator: ConstraintOperator, value: serde_json::Value) -> Vec<FieldConstraint> {
    let mut constraint = FieldConstraint::new("s");
    constraint.check = Some(ConstraintCheck {
        field: "s".to_owned(),
        operator,
        value,
        bound_expression: None,
    });
    vec![constraint]
}

#[test]
fn should_bind_an_enum_check_label_to_its_member_storage_form() {
    // Arrange
    let mut constraints = check(ConstraintOperator::NotEq, json!("b"));

    // Act
    let bound = bind(&mut constraints, &level_fields());

    // Assert
    assert!(bound.is_ok());
    assert_eq!(
        constraints[0]
            .check
            .as_ref()
            .map(|check| check.value.clone()),
        Some(json!({ crate::types::enum_type::ENUM_STORAGE_KEY: 12 }))
    );
}

#[test]
fn should_reject_an_unknown_enum_check_label_with_22p02() {
    // Arrange
    let mut constraints = check(ConstraintOperator::Eq, json!("zz"));

    // Act
    let state = bind(&mut constraints, &level_fields())
        .err()
        .map(|error| error.sql_state().to_owned());

    // Assert
    assert_eq!(state.as_deref(), Some("22P02"));
}

#[test]
fn should_reject_a_non_label_enum_check_literal_with_42883() {
    // Arrange
    let mut constraints = check(ConstraintOperator::Eq, json!(1));

    // Act
    let state = bind(&mut constraints, &level_fields())
        .err()
        .map(|error| error.sql_state().to_owned());

    // Assert
    assert_eq!(state.as_deref(), Some("42883"));
}
