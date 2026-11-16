//! Test-only `sqlx` codecs for the reserved graph types: PostgreSQL binary record and array
//! framing for values `sqlx` has no codec for, and the text form of any value (#1268).
//!
//! `sqlx` resolves a user-defined composite or array OID through `pg_type`, so a value's
//! [`PgTypeInfo`] carries the field and element types the shared framing needs. Parameters
//! are sent in binary; a [`Bound`] carries the resolved type of the statement parameter it
//! is bound to.

use crate::record_frame::{decode, encode, CodecError, Node, Shape};
use sqlx::encode::IsNull;
use sqlx::postgres::types::Oid;
use sqlx::postgres::{
    PgArgumentBuffer, PgTypeInfo, PgTypeKind, PgValueFormat, PgValueRef, Postgres,
};
use sqlx::{Decode, Encode, Type, ValueRef};

/// The framing shape of a `sqlx` type that was resolved against the server.
///
/// # Errors
///
/// An error for a type without an OID or a scalar the codec does not frame.
pub fn shape(info: &PgTypeInfo) -> Result<Shape, CodecError> {
    match info.kind() {
        PgTypeKind::Composite(fields) => Ok(Shape::Record(
            fields
                .iter()
                .map(|(_, field)| Ok((oid(field)?, shape(field)?)))
                .collect::<Result<Vec<_>, CodecError>>()?,
        )),
        PgTypeKind::Array(inner) => Ok(Shape::Array(oid(inner)?, Box::new(shape(inner)?))),
        _ => Shape::scalar(oid(info)?),
    }
}

fn oid(info: &PgTypeInfo) -> Result<u32, CodecError> {
    info.oid()
        .map(|oid| oid.0)
        .ok_or_else(|| CodecError::from("a type has no OID"))
}

/// A decoded binary value of any shape `record_frame` frames.
impl Type<Postgres> for Node {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(0))
    }

    fn compatible(_: &PgTypeInfo) -> bool {
        true
    }
}

impl<'r> Decode<'r, Postgres> for Node {
    fn decode(value: PgValueRef<'r>) -> Result<Self, CodecError> {
        if value.is_null() {
            return Ok(Node::Null);
        }
        if value.format() != PgValueFormat::Binary {
            return Err("a binary value is required".into());
        }
        let info = value.type_info().into_owned();
        decode(&shape(&info)?, value.as_bytes()?)
    }
}

/// A node bound to a statement parameter of the resolved type `info`.
pub struct Bound {
    pub node: Node,
    pub info: PgTypeInfo,
}

impl Type<Postgres> for Bound {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(0))
    }

    fn compatible(_: &PgTypeInfo) -> bool {
        true
    }
}

impl Encode<'_, Postgres> for Bound {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, CodecError> {
        if self.node == Node::Null {
            return Ok(IsNull::Yes);
        }
        let mut body = Vec::new();
        encode(&self.node, &shape(&self.info)?, &mut body)?;
        buf.extend_from_slice(&body);
        Ok(IsNull::No)
    }

    fn produces(&self) -> Option<PgTypeInfo> {
        Some(self.info.clone())
    }
}

/// The text form of a value, as the simple query protocol returns it.
pub struct TextForm(pub String);

impl Type<Postgres> for TextForm {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(0))
    }

    fn compatible(_: &PgTypeInfo) -> bool {
        true
    }
}

impl<'r> Decode<'r, Postgres> for TextForm {
    fn decode(value: PgValueRef<'r>) -> Result<Self, CodecError> {
        if value.format() != PgValueFormat::Text {
            return Err("a text value is required".into());
        }
        Ok(Self(value.as_str()?.to_owned()))
    }
}
