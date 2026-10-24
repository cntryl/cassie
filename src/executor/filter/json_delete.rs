//! Selected JSONB text-key deletion over Cassie's existing JSON carrier.
use std::{collections::BTreeMap, fmt};

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::{QueryError, ScalarValue};
use crate::app::CassieError;
use serde_json::value::RawValue;

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

pub(super) fn delete_text(
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let (ScalarValue::Json(document), ScalarValue::Str(key)) = (left, right) else {
        return Err(CassieError::Execution(
            "operator does not exist: json - incompatible operands".into(),
        )
        .into());
    };
    let value = match document.trim_start().as_bytes().first() {
        Some(b'{') => delete_object_key(document, key)?,
        Some(b'[') => delete_array_strings(document, key)?,
        _ => {
            return Err(
                CassieError::InvalidParameterValue("cannot delete from scalar".into()).into(),
            );
        }
    };
    Ok(ScalarValue::Json(value))
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
    let values = serde_json::from_str::<Vec<Box<RawValue>>>(document)
        .map_err(|error| invalid_document(&error))?;
    format_array(values, Some(key))
}

fn format_array(
    values: Vec<Box<RawValue>>,
    remove_key: Option<&str>,
) -> Result<String, QueryError> {
    let mut result = String::from("[");
    let mut retained = 0;
    for value in values {
        let matches_key = remove_key.is_some_and(|key| {
            serde_json::from_str::<String>(value.get()).is_ok_and(|value| value == key)
        });
        if matches_key {
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
