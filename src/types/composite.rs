//! Named composite types and record values (#828).
//!
//! A [`CompositeType`] is an immutable, versioned field descriptor. A
//! [`RecordValue`] pairs one descriptor with its field values and rides in
//! [`TypedValue::Record`](super::typed::TypedValue::Record). Anonymous `ROW(...)`
//! values use the transient `record` pseudo-type (OID 2249) and cannot be stored.
//!
//! This module owns the text, binary-framing, ordering-key and CRB1 payload
//! forms of a record; type-directed casts of its fields live in the executor.

mod descriptor;
mod key;
mod record;
pub mod text;
pub mod wire;

pub use descriptor::{
    CompositeField, CompositeRef, CompositeType, RECORD_ARRAY_OID, RECORD_OID, RECORD_STORAGE_KEY,
};
pub use key::{record_key, RecordKey};
pub use record::{value_data_type, RecordValue};
pub use text::{format_record, format_record_with, parse_record_text, RecordInputError};
pub use wire::{
    receive_record_binary, record_from_storage_json, record_to_storage_json, send_record_binary,
    StoredRecordError,
};

#[cfg(test)]
mod tests;
