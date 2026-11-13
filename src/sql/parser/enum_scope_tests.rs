use std::rc::Rc;

use super::{parse_enum_type, with_enum_scope, EnumResolver};
use crate::types::enum_type::{EnumMember, EnumType};
use crate::types::{DataType, EnumRef};

fn mood() -> EnumRef {
    EnumRef::new(EnumType {
        oid: 90_001,
        array_oid: 90_002,
        qualified_name: "db.public.mood".to_owned(),
        name: "mood".to_owned(),
        revision: 1,
        members: vec![EnumMember {
            member_id: 1,
            label: "sad".to_owned(),
            sort_order: 1.0,
            uncommitted: false,
        }],
    })
}

fn resolver() -> EnumResolver {
    let descriptor = mood();
    Rc::new(move |raw: &str| raw.eq_ignore_ascii_case("mood").then(|| descriptor.clone()))
}

#[test]
fn should_resolve_an_enum_name_inside_the_scope() {
    // Arrange
    let scope = resolver();

    // Act
    let parsed = with_enum_scope(scope, || parse_enum_type(" MOOD "));

    // Assert
    assert!(matches!(parsed, Some(DataType::Enum(descriptor)) if descriptor.oid == 90_001));
}

#[test]
fn should_resolve_every_array_spelling_to_enum_arrays() {
    // Arrange
    let scope = resolver();

    // Act
    let (bracket, keyword) = with_enum_scope(scope, || {
        (parse_enum_type("mood[]"), parse_enum_type("mood ARRAY"))
    });

    // Assert
    assert!(matches!(
        bracket,
        Some(DataType::Array(inner)) if matches!(*inner, DataType::Enum(_))
    ));
    assert!(matches!(
        keyword,
        Some(DataType::Array(inner)) if matches!(*inner, DataType::Enum(_))
    ));
}

#[test]
fn should_not_resolve_without_an_installed_scope() {
    // Arrange
    let text = "mood";

    // Act
    let parsed = parse_enum_type(text);

    // Assert
    assert!(parsed.is_none());
}

#[test]
fn should_restore_the_previous_scope_after_a_nested_scope() {
    // Arrange
    let empty: EnumResolver = Rc::new(|_: &str| None);

    // Act
    let after = with_enum_scope(resolver(), || {
        with_enum_scope(empty, || ());
        parse_enum_type("mood")
    });

    // Assert
    assert!(after.is_some());
}
