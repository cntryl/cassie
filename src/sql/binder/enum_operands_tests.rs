use super::*;
use crate::types::enum_type::sort_order_at;
use crate::types::{EnumMember, EnumType};

fn mood() -> EnumRef {
    EnumRef::new(EnumType {
        oid: 300_001,
        array_oid: 300_002,
        qualified_name: "db.public.mood".to_string(),
        name: "mood".to_string(),
        revision: 1,
        members: vec![EnumMember {
            member_id: 1,
            label: "sad".to_string(),
            sort_order: sort_order_at(0),
            uncommitted: false,
        }],
    })
}

#[test]
fn should_report_postgresql_names_for_a_text_operand() {
    // Arrange
    let error = undefined(
        &BinaryOp::Lt,
        "mood",
        &pg_name(&DataType::Varchar { length: None }),
    );

    // Act
    let message = error.to_string();

    // Assert
    assert!(message.contains("operator does not exist: mood < character varying"));
    assert_eq!(error.sql_state(), "42883");
}

#[test]
fn should_recognise_only_enum_typed_operands() {
    // Arrange
    let direct = DataType::Enum(mood());

    // Act
    let found = descriptor(Some(&direct));
    let absent = descriptor(Some(&DataType::Text));

    // Assert
    assert_eq!(found.map(|found| found.oid), Some(300_001));
    assert!(absent.is_none());
}

#[test]
fn should_validate_an_unknown_literal_label_when_binding_the_comparison() {
    // Arrange
    let target = mood();
    let mut valid = Expr::StringLiteral("sad".to_string());
    let mut invalid = Expr::StringLiteral("glad".to_string());

    // Act
    let accepted = coerce(&BinaryOp::Eq, &target, &mut valid, None, true);
    let rejected = coerce(&BinaryOp::Eq, &target, &mut invalid, None, true);

    // Assert
    assert!(accepted.is_ok());
    assert!(matches!(valid, Expr::Cast { .. }));
    assert!(matches!(rejected, Err(error) if error.sql_state() == "22P02"));
}

#[test]
fn should_validate_each_label_of_an_unknown_array_literal() {
    // Arrange
    let target = mood();

    // Act
    let valid = validate_array_labels(&target, "{sad,NULL}");
    let invalid = validate_array_labels(&target, "{sad,glad}");
    let malformed = validate_array_labels(&target, "sad");

    // Assert
    assert!(valid.is_ok());
    assert!(matches!(invalid, Err(error) if error.sql_state() == "22P02"));
    assert!(malformed.is_ok());
}
