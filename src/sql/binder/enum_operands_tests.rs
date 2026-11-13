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
fn should_report_postgresql_operator_and_type_names_for_a_text_operand() {
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
