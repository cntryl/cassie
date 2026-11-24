use super::search_value;
use crate::types::enum_type::{EnumMember, EnumType};
use crate::types::typed::TypedValue;
use crate::types::{EnumRef, EnumValue, SqlArray, Value};

fn letters() -> EnumRef {
    let member = |member_id, label: &str, sort_order| EnumMember {
        member_id,
        label: label.to_owned(),
        sort_order,
        uncommitted: false,
    };
    EnumRef::new(EnumType {
        oid: 50_001,
        array_oid: 50_002,
        qualified_name: "db.public.letters".to_owned(),
        name: "letters".to_owned(),
        revision: 1,
        members: vec![member(1, "a", 1.0), member(2, "b", 2.0)],
    })
}

fn array(descriptor: &EnumRef) -> SqlArray {
    let elements = ["a", "b"]
        .iter()
        .map(|label| {
            Value::Typed(TypedValue::Enum(
                EnumValue::from_label(descriptor, label).expect("member"),
            ))
        })
        .collect();
    SqlArray::from_elements(elements).expect("one-dimensional array")
}

#[test]
fn should_resolve_a_text_label_to_the_array_element_enum() {
    // Arrange
    let descriptor = letters();
    let haystack = array(&descriptor);
    let needle = Value::String("b".to_owned());

    // Act
    let resolved = search_value(&haystack, &needle).map(std::borrow::Cow::into_owned);

    // Assert
    assert_eq!(resolved.ok().as_ref(), haystack.elements().get(1));
}

#[test]
fn should_reject_a_label_that_is_not_a_member_with_22p02() {
    // Arrange
    let descriptor = letters();
    let haystack = array(&descriptor);
    let needle = Value::String("zz".to_owned());

    // Act
    let state = match search_value(&haystack, &needle) {
        Err(crate::executor::QueryError::Cassie(error)) => Some(error.sql_state()),
        _ => None,
    };

    // Assert
    assert_eq!(state, Some("22P02"));
}
