//! Searching a named enum array for an unknown-typed label (#827).
//!
//! `array_position(enum_array, 'label')` resolves the literal as the array's
//! element enum, as PostgreSQL does for `anycompatible`. Left as text it would
//! never equal a typed element and the search would silently find nothing.

use std::borrow::Cow;

use super::{QueryError, Value};
use crate::types::typed::TypedValue;
use crate::types::{EnumValue, SqlArray};

/// `search` as a member of the enum `array` holds, when `search` is a text
/// label and the array has enum elements; any other pair is returned as is.
///
/// # Errors
/// Returns `22P02` for a label that is not a member and `55P04` for a label
/// added in the open transaction.
pub(super) fn search_value<'a>(
    array: &SqlArray,
    search: &'a Value,
) -> Result<Cow<'a, Value>, QueryError> {
    let Value::String(label) = search else {
        return Ok(Cow::Borrowed(search));
    };
    let Some(descriptor) = array.elements().iter().find_map(|element| match element {
        Value::Typed(TypedValue::Enum(member)) => Some(member.descriptor()),
        _ => None,
    }) else {
        return Ok(Cow::Borrowed(search));
    };
    EnumValue::parse_label(descriptor, label)
        .map(|member| Cow::Owned(Value::Typed(TypedValue::Enum(member))))
        .map_err(|error| {
            QueryError::Cassie(crate::app::CassieError::from(
                crate::executor::filter::enum_cast::input_diagnostic(&error),
            ))
        })
}

#[cfg(test)]
#[path = "array_enum_search_tests.rs"]
mod tests;
