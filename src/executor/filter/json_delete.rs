//! Selected JSONB text-key deletion over Cassie's existing JSON carrier.
use std::{collections::BTreeMap, fmt};

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::{QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::{array, extended::TypeInputError, DataType};
use serde_json::value::RawValue;
use serde_json::Value as JsonValue;

struct RawObject(BTreeMap<String, Box<RawValue>>);

impl<'de> Deserialize<'de> for RawObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawObjectVisitor;

        impl<'de> Visitor<'de> for RawObjectVisitor {
            type Value = RawObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut fields = BTreeMap::new();
                while let Some((field, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    fields.insert(field, value);
                }
                Ok(RawObject(fields))
            }
        }

        deserializer.deserialize_map(RawObjectVisitor)
    }
}

pub(super) fn delete_operator(
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let document = json_document(left)?;
    match right {
        ScalarValue::Str(key) => delete_text(document, key),
        ScalarValue::Int(index) => delete_integer(document, *index),
        ScalarValue::Json(_) => delete_text_array(document, right),
        _ => Err(unsupported_subtraction_operand()),
    }
}

pub(super) fn delete_path(
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let document = json_document(left)?;
    let path = text_array(right)?
        .into_iter()
        .enumerate()
        .map(|(index, segment)| {
            segment.ok_or_else(|| {
                type_input_error(
                    "22004",
                    format!("path element at position {} is null", index + 1),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !matches!(document.trim_start().as_bytes().first(), Some(b'{' | b'[')) {
        return Err(invalid_parameter("cannot delete path in scalar"));
    }
    let raw = serde_json::from_str::<Box<RawValue>>(document)
        .map_err(|error| invalid_document(&error))?;
    let value = delete_json_path(&raw, &path, 1)?;
    Ok(ScalarValue::Json(value))
}

fn json_document(value: &ScalarValue) -> Result<&str, QueryError> {
    match value {
        ScalarValue::Json(document) => Ok(document),
        _ => Err(invalid_parameter("cannot delete from scalar")),
    }
}

fn delete_text(document: &str, key: &str) -> Result<ScalarValue, QueryError> {
    let value = match document.trim_start().as_bytes().first() {
        Some(b'{') => delete_object_key(document, key)?,
        Some(b'[') => delete_array_strings(document, key)?,
        _ => {
            return Err(invalid_parameter("cannot delete from scalar"));
        }
    };
    Ok(ScalarValue::Json(value))
}

fn delete_integer(document: &str, index: i64) -> Result<ScalarValue, QueryError> {
    let value = match document.trim_start().as_bytes().first() {
        Some(b'[') => {
            let values = parse_raw_array(document)?;
            let index = normalize_array_index(index, values.len());
            format_array(values, index.map(ArrayRemoval::Index).as_ref())?
        }
        Some(b'{') => {
            return Err(invalid_parameter(
                "cannot delete from object using integer index",
            ));
        }
        _ => return Err(invalid_parameter("cannot delete from scalar")),
    };
    Ok(ScalarValue::Json(value))
}

fn delete_text_array(document: &str, right: &ScalarValue) -> Result<ScalarValue, QueryError> {
    let keys = text_array(right)?;
    let keys = keys.into_iter().flatten().collect::<Vec<_>>();
    let value = match document.trim_start().as_bytes().first() {
        Some(b'{') => {
            let mut fields = serde_json::from_str::<RawObject>(document)
                .map_err(|error| invalid_document(&error))?
                .0;
            for key in &keys {
                fields.remove(key);
            }
            format_object(fields)?
        }
        Some(b'[') => format_array(parse_raw_array(document)?, Some(&ArrayRemoval::Keys(&keys)))?,
        _ => return Err(invalid_parameter("cannot delete from scalar")),
    };
    Ok(ScalarValue::Json(value))
}

fn text_array(value: &ScalarValue) -> Result<Vec<Option<String>>, QueryError> {
    let (ScalarValue::Json(text) | ScalarValue::Str(text)) = value else {
        return Err(unsupported_subtraction_operand());
    };
    let array = array::parse_text_array(text, &DataType::Text)
        .map_err(|error| QueryError::Cassie(CassieError::ArrayInput(error.to_string())))?;
    let Some(values) = array.as_array() else {
        return Err(unsupported_subtraction_operand());
    };
    values
        .iter()
        .map(|value| match value {
            JsonValue::String(value) => Ok(Some(value.clone())),
            JsonValue::Null => Ok(None),
            _ => Err(unsupported_subtraction_operand()),
        })
        .collect()
}

fn normalize_array_index(index: i64, length: usize) -> Option<usize> {
    let length = i64::try_from(length).ok()?;
    let index = if index < 0 {
        index.checked_add(length)?
    } else {
        index
    };
    if index < 0 || index >= length {
        return None;
    }
    usize::try_from(index).ok()
}

fn delete_json_path(
    value: &RawValue,
    path: &[String],
    position: usize,
) -> Result<String, QueryError> {
    let Some((head, tail)) = path.split_first() else {
        return format_raw_json(value);
    };
    match value.get().trim_start().as_bytes().first() {
        Some(b'{') => {
            let mut fields = serde_json::from_str::<RawObject>(value.get())
                .map_err(|error| invalid_document(&error))?
                .0;
            if tail.is_empty() {
                fields.remove(head);
            } else if let Some(nested) = fields.remove(head) {
                let rewritten = delete_json_path(&nested, tail, position + 1)?;
                fields.insert(head.clone(), raw_value(rewritten)?);
            }
            format_object(fields)
        }
        Some(b'[') => {
            let mut values = parse_raw_array(value.get())?;
            let Some(index) = parse_path_index(head, values.len(), position)? else {
                return format_array(values, None);
            };
            if tail.is_empty() {
                return format_array(values, Some(&ArrayRemoval::Index(index)));
            }
            let nested = &values[index];
            let rewritten = delete_json_path(nested, tail, position + 1)?;
            values[index] = raw_value(rewritten)?;
            format_array(values, None)
        }
        _ => format_raw_json(value),
    }
}

fn parse_path_index(
    element: &str,
    length: usize,
    position: usize,
) -> Result<Option<usize>, QueryError> {
    let index = element.trim().parse::<i32>().map_err(|_| {
        type_input_error(
            "22P02",
            format!("path element at position {position} is not an integer: \"{element}\""),
        )
    })?;
    Ok(normalize_array_index(i64::from(index), length))
}

fn raw_value(value: String) -> Result<Box<RawValue>, QueryError> {
    RawValue::from_string(value).map_err(|error| invalid_document(&error))
}

fn parse_raw_array(document: &str) -> Result<Vec<Box<RawValue>>, QueryError> {
    serde_json::from_str::<Vec<Box<RawValue>>>(document).map_err(|error| invalid_document(&error))
}

enum ArrayRemoval<'a> {
    Index(usize),
    Key(&'a str),
    Keys(&'a [String]),
}

fn unsupported_subtraction_operand() -> QueryError {
    CassieError::Execution("operator does not exist: json - incompatible operands".into()).into()
}

fn invalid_parameter(message: &str) -> QueryError {
    CassieError::InvalidParameterValue(message.into()).into()
}

fn type_input_error(sqlstate: &'static str, message: String) -> QueryError {
    CassieError::TypeInput(TypeInputError::new(sqlstate, message)).into()
}

fn delete_object_key(document: &str, key: &str) -> Result<String, QueryError> {
    let mut fields = serde_json::from_str::<RawObject>(document)
        .map_err(|error| invalid_document(&error))?
        .0;
    fields.remove(key);
    format_object(fields)
}

fn format_object(fields: BTreeMap<String, Box<RawValue>>) -> Result<String, QueryError> {
    let mut fields = fields.into_iter().collect::<Vec<_>>();
    fields.sort_unstable_by(|(left, _), (right, _)| {
        left.len()
            .cmp(&right.len())
            .then_with(|| left.as_bytes().cmp(right.as_bytes()))
    });
    let mut result = String::from("{");
    for (index, (field, value)) in fields.into_iter().enumerate() {
        if index > 0 {
            result.push_str(", ");
        }
        result.push_str(&serde_json::to_string(&field).map_err(|error| invalid_document(&error))?);
        result.push_str(": ");
        result.push_str(&format_raw_json(&value)?);
    }
    result.push('}');
    Ok(result)
}

fn delete_array_strings(document: &str, key: &str) -> Result<String, QueryError> {
    format_array(parse_raw_array(document)?, Some(&ArrayRemoval::Key(key)))
}

fn format_array(
    values: Vec<Box<RawValue>>,
    removal: Option<&ArrayRemoval<'_>>,
) -> Result<String, QueryError> {
    let mut result = String::from("[");
    let mut retained = 0;
    for (index, value) in values.into_iter().enumerate() {
        if removal.is_some_and(|removal| match removal {
            ArrayRemoval::Index(remove_index) => index == *remove_index,
            ArrayRemoval::Key(key) => json_string_matches(&value, key),
            ArrayRemoval::Keys(keys) => keys.iter().any(|key| json_string_matches(&value, key)),
        }) {
            continue;
        }
        if retained > 0 {
            result.push_str(", ");
        }
        result.push_str(&format_raw_json(&value)?);
        retained += 1;
    }
    result.push(']');
    Ok(result)
}

fn json_string_matches(value: &RawValue, expected: &str) -> bool {
    serde_json::from_str::<String>(value.get()).is_ok_and(|value| value == expected)
}

fn format_raw_json(value: &RawValue) -> Result<String, QueryError> {
    let token = value.get();
    match token.trim_start().as_bytes().first() {
        Some(b'{') => {
            let fields = serde_json::from_str::<RawObject>(token)
                .map_err(|error| invalid_document(&error))?
                .0;
            format_object(fields)
        }
        Some(b'[') => {
            let values = serde_json::from_str::<Vec<Box<RawValue>>>(token)
                .map_err(|error| invalid_document(&error))?;
            format_array(values, None)
        }
        Some(b'"') => {
            let text =
                serde_json::from_str::<String>(token).map_err(|error| invalid_document(&error))?;
            serde_json::to_string(&text).map_err(|error| invalid_document(&error))
        }
        Some(b'-' | b'0'..=b'9') => crate::types::extended::numeric::parse_text(token)
            .map(|number| number.to_string())
            .map_err(|error| QueryError::General(error.to_string())),
        Some(b't' | b'f' | b'n') => Ok(token.to_owned()),
        _ => Err(QueryError::General("invalid JSON value".into())),
    }
}

fn invalid_document(error: &serde_json::Error) -> QueryError {
    QueryError::General(format!("invalid JSON document: {error}"))
}
