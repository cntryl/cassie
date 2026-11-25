//! Ordered raw JSON nodes for lossless null stripping and canonical pretty output.
use crate::{app::CassieError, executor::QueryError, runtime::QueryExecutionControls};
use serde::de::{MapAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use std::{
    fmt,
    io::{self, Write},
};

pub(super) enum Node {
    Scalar(Box<RawValue>),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}

impl Node {
    pub(super) fn parse(text: &str) -> Result<Self, QueryError> {
        let raw = serde_json::from_str(text).map_err(|error| input_error(&error))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: Box<RawValue>) -> Result<Self, QueryError> {
        match raw.get().trim_start().as_bytes().first() {
            Some(b'{') => {
                let fields = serde_json::from_str::<OrderedObject>(raw.get())
                    .map_err(|error| input_error(&error))?
                    .0;
                drop(raw);
                let mut nodes = Vec::with_capacity(fields.len());
                for (key, value) in fields {
                    nodes.push((key, Self::from_raw(value)?));
                }
                Ok(Self::Object(nodes))
            }
            Some(b'[') => {
                let values = serde_json::from_str::<Vec<Box<RawValue>>>(raw.get())
                    .map_err(|error| input_error(&error))?;
                drop(raw);
                let mut nodes = Vec::with_capacity(values.len());
                for value in values {
                    nodes.push(Self::from_raw(value)?);
                }
                Ok(Self::Array(nodes))
            }
            _ => Ok(Self::Scalar(raw)),
        }
    }

    fn is_null(&self) -> bool {
        matches!(self, Self::Scalar(value) if value.get().trim() == "null")
    }

    pub(super) fn strip_nulls(
        &mut self,
        arrays: bool,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), QueryError> {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        match self {
            Self::Object(fields) => {
                fields.retain(|(_, value)| !value.is_null());
                for (_, value) in fields {
                    value.strip_nulls(arrays, controls)?;
                }
            }
            Self::Array(values) => {
                if arrays {
                    values.retain(|value| !value.is_null());
                }
                for value in values {
                    value.strip_nulls(arrays, controls)?;
                }
            }
            Self::Scalar(_) => {}
        }
        Ok(())
    }

    pub(super) fn render(
        &self,
        pretty: bool,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<String, QueryError> {
        let mut counter = Counter(0);
        self.write(&mut counter, pretty)
            .map_err(|_| CassieError::ResourceLimit("JSON output size overflow".into()))?;
        let scratch = crate::executor::retained_memory::mul(counter.0.max(8), 4)?;
        let _memory = controls
            .map(|controls| controls.reserve_query_memory(scratch))
            .transpose()?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(counter.0)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if bytes.capacity() > counter.0 {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted JSON output capacity".into(),
            )
            .into());
        }
        self.write(&mut bytes, pretty)
            .map_err(|error| input_error(&error))?;
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        String::from_utf8(bytes).map_err(|error| QueryError::General(error.to_string()))
    }

    fn write(&self, output: &mut impl Write, pretty: bool) -> Result<(), serde_json::Error> {
        if pretty {
            let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
            self.serialize(&mut serde_json::Serializer::with_formatter(
                output, formatter,
            ))
        } else {
            self.serialize(&mut serde_json::Serializer::new(output))
        }
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Scalar(value) => value.serialize(serializer),
            Self::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(value)?;
                }
                sequence.end()
            }
            Self::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

struct OrderedObject(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for OrderedObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = OrderedObject;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(OrderedObject(fields))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("JSON output overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn input_error(error: &serde_json::Error) -> QueryError {
    QueryError::General(format!("invalid JSON document: {error}"))
}
