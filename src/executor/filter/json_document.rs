//! Raw JSON document parsing and PostgreSQL JSONB text formatting.
use std::{collections::BTreeMap, fmt};

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

use super::QueryError;

pub(super) struct RawObject(pub(super) BTreeMap<String, Box<RawValue>>);

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

pub(super) fn parse_object(document: &str) -> Result<RawObject, QueryError> {
    serde_json::from_str::<RawObject>(document).map_err(|error| invalid_document(&error))
}

pub(super) fn object_field(
    document: &str,
    requested: &str,
) -> Result<Option<Box<RawValue>>, QueryError> {
    struct FieldVisitor<'a>(&'a str);

    impl<'de> Visitor<'de> for FieldVisitor<'_> {
        type Value = Option<Box<RawValue>>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a JSON object")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut selected = None;
            while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                if key == self.0 {
                    selected = Some(value);
                }
            }
            Ok(selected)
        }
    }

    serde_json::Deserializer::from_str(document)
        .deserialize_map(FieldVisitor(requested))
        .map_err(|error| invalid_document(&error))
}

pub(super) fn parse_array(document: &str) -> Result<Vec<Box<RawValue>>, QueryError> {
    serde_json::from_str::<Vec<Box<RawValue>>>(document).map_err(|error| invalid_document(&error))
}

pub(super) fn parse_value(document: &str) -> Result<Box<RawValue>, QueryError> {
    serde_json::from_str::<Box<RawValue>>(document).map_err(|error| invalid_document(&error))
}

pub(super) fn raw_value(value: String) -> Result<Box<RawValue>, QueryError> {
    RawValue::from_string(value).map_err(|error| invalid_document(&error))
}

pub(super) fn format_object(fields: BTreeMap<String, Box<RawValue>>) -> Result<String, QueryError> {
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
        result.push_str(&format_value(&value)?);
    }
    result.push('}');
    Ok(result)
}

pub(super) fn format_array(values: Vec<Box<RawValue>>) -> Result<String, QueryError> {
    let mut result = String::from("[");
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            result.push_str(", ");
        }
        result.push_str(&format_value(&value)?);
    }
    result.push(']');
    Ok(result)
}

pub(super) fn format_value(value: &RawValue) -> Result<String, QueryError> {
    let token = value.get();
    match token.trim_start().as_bytes().first() {
        Some(b'{') => format_object(parse_object(token)?.0),
        Some(b'[') => format_array(parse_array(token)?),
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
