//! Test-only PostgreSQL binary framing for composite records and one-dimensional arrays, so
//! `tokio-postgres` can send and receive user-defined composite OIDs it has no codec for.
//!
//! The wire layout is the documented `record_send`/`array_send` framing: a record is a field
//! count and then `(type OID, length, bytes)` per field; an array is `(ndim, has-null flag,
//! element OID)`, a `(size, lower bound)` pair per dimension and `(length, bytes)` elements.
//! A length of -1 is NULL.

use bytes::{BufMut, BytesMut};
use std::error::Error;
use tokio_postgres::types::{to_sql_checked, FromSql, IsNull, Kind, ToSql, Type};

type CodecError = Box<dyn Error + Sync + Send>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Null,
    Int2(i16),
    Int4(i32),
    Int8(i64),
    Text(String),
    Uuid([u8; 16]),
    Record(Vec<Node>),
    Array(Vec<Node>),
}

fn fail(message: &str) -> CodecError {
    message.into()
}

fn take<'a>(buffer: &mut &'a [u8], count: usize) -> Result<&'a [u8], CodecError> {
    if buffer.len() < count {
        return Err(fail("truncated binary value"));
    }
    let (head, tail) = buffer.split_at(count);
    *buffer = tail;
    Ok(head)
}

fn take_i32(buffer: &mut &[u8]) -> Result<i32, CodecError> {
    let bytes = take(buffer, 4)?;
    Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn element(buffer: &mut &[u8], ty: &Type) -> Result<Node, CodecError> {
    let length = take_i32(buffer)?;
    if length < 0 {
        return Ok(Node::Null);
    }
    let length = usize::try_from(length)?;
    decode(ty, take(buffer, length)?)
}

fn decode(ty: &Type, raw: &[u8]) -> Result<Node, CodecError> {
    let mut buffer = raw;
    match ty.kind() {
        Kind::Composite(fields) => {
            if usize::try_from(take_i32(&mut buffer)?)? != fields.len() {
                return Err(fail("record field count mismatch"));
            }
            let mut nodes = Vec::new();
            for field in fields {
                if u32::try_from(take_i32(&mut buffer)?)? != field.type_().oid() {
                    return Err(fail("record field type mismatch"));
                }
                nodes.push(element(&mut buffer, field.type_())?);
            }
            Ok(Node::Record(nodes))
        }
        Kind::Array(inner) => {
            let dimensions = take_i32(&mut buffer)?;
            take_i32(&mut buffer)?;
            if u32::try_from(take_i32(&mut buffer)?)? != inner.oid() {
                return Err(fail("array element type mismatch"));
            }
            if dimensions == 0 {
                return Ok(Node::Array(Vec::new()));
            }
            if dimensions != 1 {
                return Err(fail("only one-dimensional arrays are used"));
            }
            let size = usize::try_from(take_i32(&mut buffer)?)?;
            take_i32(&mut buffer)?;
            let mut nodes = Vec::new();
            for _ in 0..size {
                nodes.push(element(&mut buffer, inner)?);
            }
            Ok(Node::Array(nodes))
        }
        _ if *ty == Type::INT2 => Ok(Node::Int2(i16::from_be_bytes(raw.try_into()?))),
        _ if *ty == Type::INT4 => Ok(Node::Int4(i32::from_be_bytes(raw.try_into()?))),
        _ if *ty == Type::INT8 => Ok(Node::Int8(i64::from_be_bytes(raw.try_into()?))),
        _ if *ty == Type::TEXT => Ok(Node::Text(std::str::from_utf8(raw)?.to_owned())),
        _ if *ty == Type::UUID => Ok(Node::Uuid(raw.try_into()?)),
        _ => Err(fail("unsupported scalar type")),
    }
}

fn put_length_prefixed(out: &mut BytesMut, node: &Node, ty: &Type) -> Result<(), CodecError> {
    if *node == Node::Null {
        out.put_i32(-1);
        return Ok(());
    }
    let mark = out.len();
    out.put_i32(0);
    encode(node, ty, out)?;
    let length = i32::try_from(out.len() - mark - 4)?;
    out[mark..mark + 4].copy_from_slice(&length.to_be_bytes());
    Ok(())
}

fn encode(node: &Node, ty: &Type, out: &mut BytesMut) -> Result<(), CodecError> {
    match (node, ty.kind()) {
        (Node::Record(nodes), Kind::Composite(fields)) if nodes.len() == fields.len() => {
            out.put_i32(i32::try_from(nodes.len())?);
            for (node, field) in nodes.iter().zip(fields) {
                out.put_u32(field.type_().oid());
                put_length_prefixed(out, node, field.type_())?;
            }
        }
        (Node::Array(nodes), Kind::Array(inner)) => {
            out.put_i32(i32::from(!nodes.is_empty()));
            out.put_i32(i32::from(nodes.contains(&Node::Null)));
            out.put_u32(inner.oid());
            if !nodes.is_empty() {
                out.put_i32(i32::try_from(nodes.len())?);
                out.put_i32(1);
                for node in nodes {
                    put_length_prefixed(out, node, inner)?;
                }
            }
        }
        (Node::Int2(value), _) => out.put_i16(*value),
        (Node::Int4(value), _) => out.put_i32(*value),
        (Node::Int8(value), _) => out.put_i64(*value),
        (Node::Text(value), _) => out.put_slice(value.as_bytes()),
        (Node::Uuid(value), _) => out.put_slice(value),
        _ => return Err(fail("a node does not match its declared type")),
    }
    Ok(())
}

impl ToSql for Node {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, CodecError> {
        if *self == Node::Null {
            return Ok(IsNull::Yes);
        }
        encode(self, ty, out)?;
        Ok(IsNull::No)
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for Node {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, CodecError> {
        decode(ty, raw)
    }

    fn from_sql_null(_: &Type) -> Result<Self, CodecError> {
        Ok(Node::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}
