//! Conversion of JSON values into typed record fields (`populate_record` semantics).
//!
//! A missing key keeps the base value, a JSON `null` is SQL NULL, nested objects fill
//! composite fields, nested arrays fill array fields, and every other value is converted
//! through the text input of the target type.
use super::source_json_fragment::{Fragment, Kind};
use super::{QueryError, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::types::typed::TypedValue;
use crate::types::{CompositeRef, DataType, RecordValue, SqlArray};

fn diagnostic(state: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::from(CassieError::from(DiagnosticError::new(state, message)))
}

/// The error for a top-level document that is not a JSON object.
pub(super) fn not_an_object(kind: Kind) -> QueryError {
    diagnostic(
        "22023",
        format!(
            "cannot call populate_composite on {}",
            if kind == Kind::Array {
                "an array"
            } else {
                "a scalar"
            }
        ),
    )
}

/// The base record's field values when it is a non-NULL record of the expected width.
pub(super) fn base_fields(base: Option<&Value>, width: usize) -> Option<&[Value]> {
    match base {
        Some(Value::Typed(TypedValue::Record(record))) if record.fields().len() == width => {
            Some(record.fields())
        }
        _ => None,
    }
}

/// Values of `fields` taken from the last object member with the same (case-sensitive) name.
pub(super) fn field_values(
    members: Vec<(String, Fragment)>,
    fields: &[(&str, &DataType)],
    base: Option<&[Value]>,
) -> Result<Vec<Value>, QueryError> {
    let mut members = members
        .into_iter()
        .map(|(key, fragment)| (key, Some(fragment)))
        .collect::<Vec<_>>();
    let mut values = Vec::with_capacity(fields.len());
    for (index, (name, data_type)) in fields.iter().enumerate() {
        let base_value = base.and_then(|values| values.get(index));
        let found = members
            .iter()
            .rposition(|(key, fragment)| key == *name && fragment.is_some());
        let fragment = found.and_then(|position| members[position].1.take());
        values.push(match fragment {
            Some(fragment) => convert(fragment, data_type, base_value, name)?,
            None => base_value.cloned().unwrap_or(Value::Null),
        });
    }
    Ok(values)
}

/// Converts one JSON value into a value of `target`. `hint` names the key for diagnostics.
pub(super) fn convert(
    fragment: Fragment,
    target: &DataType,
    base: Option<&Value>,
    hint: &str,
) -> Result<Value, QueryError> {
    if fragment.kind() == Kind::Null {
        return Ok(Value::Null);
    }
    match target {
        DataType::Json => Ok(Value::String(fragment.json_text()?)),
        DataType::Jsonb => Ok(Value::Json(fragment.tree()?)),
        DataType::Composite(descriptor) => composite(fragment, descriptor, base),
        DataType::Array(element) => array(fragment, target, element, hint),
        _ => scalar(&fragment, target),
    }
}

/// Input-function semantics of `varchar(n)` and `char(n)`: too long is an error, never a
/// truncation (explicit casts truncate, assignments and input do not).
pub(super) fn check_length(text: &str, target: &DataType) -> Result<(), QueryError> {
    let (limit, name) = match target {
        DataType::Varchar {
            length: Some(limit),
        } => (*limit, "character varying"),
        DataType::Char {
            length: Some(limit),
        } => (*limit, "character"),
        _ => return Ok(()),
    };
    let allowed = usize::try_from(limit).unwrap_or(usize::MAX);
    // Trailing blanks never count against the limit; the later cast removes them.
    let counted = text.trim_end_matches(' ').chars().count();
    if counted > allowed {
        return Err(diagnostic(
            "22001",
            format!("value too long for type {name}({limit})"),
        ));
    }
    Ok(())
}

fn scalar(fragment: &Fragment, target: &DataType) -> Result<Value, QueryError> {
    let text = match fragment.kind() {
        Kind::Array | Kind::Object => fragment.json_text()?,
        _ => fragment.scalar_text()?,
    };
    if matches!(target, DataType::Text) {
        return Ok(Value::String(text));
    }
    check_length(&text, target)?;
    super::filter::cast_value(&Value::String(text), target)
}

fn composite(
    fragment: Fragment,
    descriptor: &CompositeRef,
    base: Option<&Value>,
) -> Result<Value, QueryError> {
    match fragment.kind() {
        Kind::Object => {
            let members = fragment.members()?;
            let fields = descriptor
                .fields
                .iter()
                .map(|field| (field.name.as_str(), &field.data_type))
                .collect::<Vec<_>>();
            let base = base_fields(base, fields.len());
            let values = field_values(members, &fields, base)?;
            let record = RecordValue::new(descriptor.clone(), values)
                .map_err(|message| diagnostic("XX000", message))?;
            Ok(Value::Typed(TypedValue::Record(record)))
        }
        Kind::String => super::filter::cast_value(
            &Value::String(fragment.scalar_text()?),
            &DataType::Composite(descriptor.clone()),
        ),
        kind => Err(not_an_object(kind)),
    }
}

fn array(
    fragment: Fragment,
    target: &DataType,
    element: &DataType,
    hint: &str,
) -> Result<Value, QueryError> {
    match fragment.kind() {
        Kind::Array => array_elements(fragment, element, hint, None).map(|(value, _)| value),
        Kind::String => super::filter::cast_value(&Value::String(fragment.scalar_text()?), target),
        _ => {
            let error = DiagnosticError::new("22P02", "expected JSON array")
                .with_hint(format!("See the value of key \"{hint}\"."));
            Err(QueryError::from(CassieError::from(error)))
        }
    }
}

/// Builds a rectangular array and reports its number of dimension levels. For JSON-typed
/// elements the first item's leading path fixes the depth (`levels`, once known); below that
/// depth an item is a leaf JSON value even when it is an array.
fn array_elements(
    fragment: Fragment,
    element: &DataType,
    hint: &str,
    levels: Option<usize>,
) -> Result<(Value, usize), QueryError> {
    let items = fragment.elements()?;
    let json_elements = matches!(element, DataType::Json | DataType::Jsonb);
    let first_is_array = items.first().is_some_and(|item| item.kind() == Kind::Array);
    let descend = match levels {
        Some(depth) => json_elements && depth > 1,
        None => !json_elements || first_is_array,
    };
    let mut known = levels.map(|depth| depth.saturating_sub(1));
    let mut depth = 1;
    let mut values = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        if descend && item.kind() == Kind::Array {
            let (value, child) = array_elements(item, element, hint, known)?;
            known = json_elements.then_some(child);
            depth = child + 1;
            values.push(value);
        } else if descend && json_elements {
            let error = DiagnosticError::new("22P02", "expected JSON array").with_hint(format!(
                "See the array element [{index}] of key \"{hint}\"."
            ));
            return Err(QueryError::from(CassieError::from(error)));
        } else {
            values.push(convert(item, element, None, "")?);
        }
    }
    // Sub-arrays that are all empty collapse to an empty array without dimensions.
    if !values.is_empty()
        && values
            .iter()
            .all(|value| matches!(value, Value::Array(array) if array.is_empty()))
    {
        values.clear();
    }
    SqlArray::from_nested_values(values)
        .map(|array| (Value::Array(array), depth))
        .map_err(|_| {
            let error = DiagnosticError::new("22P02", "malformed JSON array").with_detail(
                "Multidimensional arrays must have sub-arrays with matching dimensions.",
            );
            QueryError::from(CassieError::from(error))
        })
}
