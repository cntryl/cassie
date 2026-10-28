//! JSON duplicate-key validation for the selected SQL/JSON constructor.
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;
use std::fmt;

pub(super) fn validate(input: &str) -> Result<(), String> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    UniqueValue::deserialize(&mut deserializer).map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())
}

struct UniqueValue;

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a valid JSON value with unique object keys")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<UniqueValue>()?.is_some() {}
        Ok(UniqueValue)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(serde::de::Error::custom("duplicate JSON object key value"));
            }
            map.next_value::<UniqueValue>()?;
        }
        Ok(UniqueValue)
    }
}
