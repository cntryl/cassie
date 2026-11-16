//! Test-only PostgreSQL binary framing for composite records and one-dimensional arrays, so
//! `tokio-postgres` can send and receive user-defined composite OIDs it has no codec for.
//! The framing itself is driver-neutral and lives in `record_frame`.

use crate::record_frame::{decode, encode, CodecError, Shape};
use bytes::BytesMut;
use tokio_postgres::types::{to_sql_checked, FromSql, IsNull, Kind, ToSql, Type};

pub use crate::record_frame::Node;

/// The framing shape of a `tokio-postgres` type.
fn shape(ty: &Type) -> Result<Shape, CodecError> {
    match ty.kind() {
        Kind::Composite(fields) => Ok(Shape::Record(
            fields
                .iter()
                .map(|field| Ok((field.type_().oid(), shape(field.type_())?)))
                .collect::<Result<Vec<_>, CodecError>>()?,
        )),
        Kind::Array(inner) => Ok(Shape::Array(inner.oid(), Box::new(shape(inner)?))),
        _ => Shape::scalar(ty.oid()),
    }
}

impl ToSql for Node {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, CodecError> {
        if *self == Node::Null {
            return Ok(IsNull::Yes);
        }
        let mut body = Vec::new();
        encode(self, &shape(ty)?, &mut body)?;
        out.extend_from_slice(&body);
        Ok(IsNull::No)
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for Node {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, CodecError> {
        decode(&shape(ty)?, raw)
    }

    fn from_sql_null(_: &Type) -> Result<Self, CodecError> {
        Ok(Node::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}
