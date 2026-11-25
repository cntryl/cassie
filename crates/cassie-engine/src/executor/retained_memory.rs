use cassie_core::request::accounted::retained;

use crate::app::CassieError;
use crate::types::{DataType, Value};

/// Heap retained by cloning an owned SQL value, in addition to its inline slot.
#[doc(hidden)]
pub fn value_clone_bytes(value: &Value) -> Result<usize, CassieError> {
    retained::value_clone_bytes(value).map_err(Into::into)
}

pub(crate) fn data_type_clone_bytes(data_type: &DataType) -> Result<usize, CassieError> {
    retained::data_type_clone_bytes(data_type).map_err(Into::into)
}

/// Counts the existing JSON serializer output without constructing its output buffer.
#[doc(hidden)]
pub fn serialized_json_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    retained::serialized_json_bytes(value).map_err(Into::into)
}

pub(crate) fn lookup_bytes(entries: usize, names_bytes: usize) -> Result<usize, CassieError> {
    retained::lookup_bytes(entries, names_bytes).map_err(Into::into)
}

pub(crate) fn hash_table_bytes<T>(entries: usize) -> Result<usize, CassieError> {
    retained::hash_table_bytes::<T>(entries).map_err(Into::into)
}

pub(crate) fn grown_capacity(count: usize, minimum: usize) -> Result<usize, CassieError> {
    retained::grown_capacity(count, minimum).map_err(Into::into)
}

#[doc(hidden)]
pub fn add(left: usize, right: usize) -> Result<usize, CassieError> {
    retained::add(left, right).map_err(Into::into)
}

#[doc(hidden)]
pub fn mul(left: usize, right: usize) -> Result<usize, CassieError> {
    retained::mul(left, right).map_err(Into::into)
}
