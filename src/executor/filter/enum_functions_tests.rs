use super::*;
use crate::types::enum_type::sort_order_at;
use crate::types::{EnumMember, EnumType};

fn mood(labels: &[&str]) -> EnumRef {
    EnumRef::new(EnumType {
        oid: 300_001,
        array_oid: 300_002,
        qualified_name: "db.public.mood".to_string(),
        name: "mood".to_string(),
        revision: 1,
        members: labels
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

fn labels(value: &Value) -> Vec<String> {
    let Value::Array(array) = value else {
        panic!("expected an array");
    };
    array
        .elements()
        .iter()
        .map(|element| match element {
            Value::Typed(TypedValue::Enum(value)) => value.label().to_owned(),
            _ => panic!("expected enum elements"),
        })
        .collect()
}

fn member(descriptor: &EnumRef, label: &str) -> EnumValue {
    EnumValue::from_label(descriptor, label).expect("label")
}

#[test]
fn should_return_the_first_and_last_members_in_sort_order() {
    // Arrange
    let descriptor = mood(&["sad", "ok", "happy"]);

    // Act
    let first = boundary(&descriptor, true).expect("first");
    let last = boundary(&descriptor, false).expect("last");

    // Assert
    assert!(matches!(&first, Value::Typed(TypedValue::Enum(v)) if v.label() == "sad"));
    assert!(matches!(&last, Value::Typed(TypedValue::Enum(v)) if v.label() == "happy"));
}

#[test]
fn should_reject_first_and_last_of_an_empty_enum_with_55000() {
    // Arrange
    let descriptor = mood(&[]);

    // Act
    let first = boundary(&descriptor, true);
    let last = boundary(&descriptor, false);

    // Assert
    assert!(matches!(&first, Err(QueryError::Cassie(error)) if error.sql_state() == "55000"));
    assert!(matches!(&last, Err(QueryError::Cassie(error)) if error.sql_state() == "55000"));
}

#[test]
fn should_return_the_whole_range_when_both_bounds_are_open() {
    // Arrange
    let descriptor = mood(&["sad", "ok", "happy"]);

    // Act
    let all = range(&descriptor, None, None).expect("range");

    // Assert
    assert_eq!(labels(&all), vec!["sad", "ok", "happy"]);
}

#[test]
fn should_treat_a_null_bound_as_open_ended() {
    // Arrange
    let descriptor = mood(&["sad", "ok", "happy"]);
    let ok = member(&descriptor, "ok");

    // Act
    let from = range(&descriptor, Some(&ok), None).expect("from ok");
    let to = range(&descriptor, None, Some(&ok)).expect("to ok");

    // Assert
    assert_eq!(labels(&from), vec!["ok", "happy"]);
    assert_eq!(labels(&to), vec!["sad", "ok"]);
}

#[test]
fn should_return_an_empty_array_when_the_lower_bound_follows_the_upper() {
    // Arrange
    let descriptor = mood(&["sad", "ok", "happy"]);
    let happy = member(&descriptor, "happy");
    let sad = member(&descriptor, "sad");

    // Act
    let reversed = range(&descriptor, Some(&happy), Some(&sad)).expect("reversed range");

    // Assert
    assert_eq!(reversed, Value::Array(SqlArray::empty()));
}
