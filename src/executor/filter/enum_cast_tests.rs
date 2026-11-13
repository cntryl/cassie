use super::*;
use crate::types::enum_type::sort_order_at;
use crate::types::{EnumMember, EnumType};

fn mood_with(labels: &[(&str, bool)], oid: u32) -> EnumRef {
    EnumRef::new(EnumType {
        oid,
        array_oid: oid + 1,
        qualified_name: format!("db.public.mood_{oid}"),
        name: format!("mood_{oid}"),
        revision: 1,
        members: labels
            .iter()
            .enumerate()
            .map(|(position, (label, uncommitted))| EnumMember {
                member_id: u64::from(oid) * 10 + u64::try_from(position).expect("small position"),
                label: (*label).to_string(),
                sort_order: sort_order_at(position),
                uncommitted: *uncommitted,
            })
            .collect(),
    })
}

fn mood() -> EnumRef {
    mood_with(&[("sad", false), ("ok", false), ("happy", false)], 300_001)
}

fn state_of(error: &QueryError) -> &'static str {
    match error {
        QueryError::Cassie(error) => error.sql_state(),
        _ => "not a cassie error",
    }
}

#[test]
fn should_parse_a_label_into_the_enum_value() {
    // Arrange
    let descriptor = mood();

    // Act
    let value = cast_scalar(
        &ScalarValue::Str("ok".to_string()),
        &DataType::Enum(descriptor.clone()),
        None,
    )
    .expect("cast label");

    // Assert
    let ScalarValue::Typed(TypedValue::Enum(parsed)) = value else {
        panic!("expected an enum value");
    };
    assert_eq!(parsed.label(), "ok");
    assert_eq!(parsed.member_id(), 3_000_011);
}

#[test]
fn should_reject_an_unknown_label_with_22p02() {
    // Arrange
    let descriptor = mood();

    // Act
    let error = cast_text_to_enum(&descriptor, "furious").expect_err("unknown label");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_reject_an_uncommitted_label_with_a_hinted_55p04() {
    // Arrange
    let descriptor = mood_with(&[("sad", false), ("new", true)], 300_011);

    // Act
    let error = cast_text_to_enum(&descriptor, "new").expect_err("uncommitted label");

    // Assert
    assert_eq!(error.sql_state(), "55P04");
}

#[test]
fn should_cast_an_enum_to_character_types_as_its_label() {
    // Arrange
    let descriptor = mood();
    let value = cast_text_to_enum(&descriptor, "happy").expect("label");
    let scalar = scalar_enum(&value);

    // Act
    let text = cast_scalar(&scalar, &DataType::Text, None).expect("text");
    let varchar = cast_scalar(&scalar, &DataType::Varchar { length: None }, None).expect("varchar");
    let name = cast_scalar(&scalar, &DataType::Name, None).expect("name");

    // Assert
    for cast in [text, varchar, name] {
        assert!(matches!(&cast, ScalarValue::Str(label) if label == "happy"));
    }
}

#[test]
fn should_keep_the_value_when_casting_an_enum_to_its_own_type() {
    // Arrange
    let descriptor = mood();
    let scalar = scalar_enum(&cast_text_to_enum(&descriptor, "sad").expect("label"));

    // Act
    let cast = cast_scalar(&scalar, &DataType::Enum(descriptor), None).expect("identity cast");

    // Assert
    let (ScalarValue::Typed(TypedValue::Enum(kept)), ScalarValue::Typed(TypedValue::Enum(source))) =
        (&cast, &scalar)
    else {
        panic!("expected enum values");
    };
    assert_eq!(kept.member_id(), source.member_id());
}

#[test]
fn should_reject_non_text_casts_of_an_enum_with_42846() {
    // Arrange
    let descriptor = mood();
    let other = mood_with(&[("sad", false)], 300_021);
    let scalar = scalar_enum(&cast_text_to_enum(&descriptor, "sad").expect("label"));

    // Act
    let to_integer = cast_scalar(&scalar, &DataType::Int, None).expect_err("enum to integer");
    let from_integer =
        cast_scalar(&ScalarValue::Int(1), &DataType::Enum(descriptor), None).expect_err("integer");
    let to_other = cast_scalar(&scalar, &DataType::Enum(other), None).expect_err("other enum");

    // Assert
    assert_eq!(state_of(&to_integer), "42846");
    assert_eq!(state_of(&from_integer), "42846");
    assert_eq!(state_of(&to_other), "42846");
}

#[test]
fn should_cast_an_array_literal_to_an_enum_array() {
    // Arrange
    let descriptor = mood();
    let target = DataType::Array(Box::new(DataType::Enum(descriptor.clone())));

    // Act
    let cast = cast_scalar(&ScalarValue::Str("{sad,happy}".to_string()), &target, None)
        .expect("array literal");

    // Assert
    let ScalarValue::Array(array) = cast else {
        panic!("expected an array");
    };
    assert_eq!(array.len(), 2);
    let Value::Typed(TypedValue::Enum(second)) = &array.elements()[1] else {
        panic!("expected an enum element");
    };
    assert_eq!(second.label(), "happy");
}
