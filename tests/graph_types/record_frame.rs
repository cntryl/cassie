//! Driver-neutral PostgreSQL binary framing for composite records and one-dimensional
//! arrays, shared by the `tokio-postgres` and `sqlx` test drivers (#1268).
//!
//! The wire layout is the documented `record_send`/`array_send` framing: a record is a field
//! count and then `(type OID, length, bytes)` per field; an array is `(ndim, has-null flag,
//! element OID)`, a `(size, lower bound)` pair per dimension and `(length, bytes)` elements.
//! A length of -1 is NULL. A driver describes the type it was told about as a [`Shape`].

use std::error::Error;

pub type CodecError = Box<dyn Error + Sync + Send>;

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

/// The type of a value as a driver reports it: the OID of each record field and array
/// element travels inside the value, so the shape carries it too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Int2,
    Int4,
    Int8,
    Text,
    Uuid,
    Record(Vec<(u32, Shape)>),
    Array(u32, Box<Shape>),
}

impl Shape {
    /// The shape of the builtin scalar type with OID `oid`.
    ///
    /// # Errors
    ///
    /// An error for an OID the codec does not frame.
    pub fn scalar(oid: u32) -> Result<Self, CodecError> {
        match oid {
            21 => Ok(Self::Int2),
            23 => Ok(Self::Int4),
            20 => Ok(Self::Int8),
            25 => Ok(Self::Text),
            2950 => Ok(Self::Uuid),
            _ => Err(fail("unsupported scalar type")),
        }
    }
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

fn element(buffer: &mut &[u8], shape: &Shape) -> Result<Node, CodecError> {
    let length = take_i32(buffer)?;
    if length < 0 {
        return Ok(Node::Null);
    }
    let length = usize::try_from(length)?;
    decode(shape, take(buffer, length)?)
}

fn decode_record(fields: &[(u32, Shape)], mut buffer: &[u8]) -> Result<Node, CodecError> {
    if usize::try_from(take_i32(&mut buffer)?)? != fields.len() {
        return Err(fail("record field count mismatch"));
    }
    let mut nodes = Vec::new();
    for (oid, shape) in fields {
        if u32::try_from(take_i32(&mut buffer)?)? != *oid {
            return Err(fail("record field type mismatch"));
        }
        nodes.push(element(&mut buffer, shape)?);
    }
    Ok(Node::Record(nodes))
}

fn decode_array(oid: u32, inner: &Shape, mut buffer: &[u8]) -> Result<Node, CodecError> {
    let dimensions = take_i32(&mut buffer)?;
    take_i32(&mut buffer)?;
    if u32::try_from(take_i32(&mut buffer)?)? != oid {
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

/// Decodes the binary body `raw` of a value of type `shape`.
///
/// # Errors
///
/// An error for a truncated or mismatched body.
pub fn decode(shape: &Shape, raw: &[u8]) -> Result<Node, CodecError> {
    match shape {
        Shape::Record(fields) => decode_record(fields, raw),
        Shape::Array(oid, inner) => decode_array(*oid, inner, raw),
        Shape::Int2 => Ok(Node::Int2(i16::from_be_bytes(raw.try_into()?))),
        Shape::Int4 => Ok(Node::Int4(i32::from_be_bytes(raw.try_into()?))),
        Shape::Int8 => Ok(Node::Int8(i64::from_be_bytes(raw.try_into()?))),
        Shape::Text => Ok(Node::Text(std::str::from_utf8(raw)?.to_owned())),
        Shape::Uuid => Ok(Node::Uuid(raw.try_into()?)),
    }
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_length_prefixed(out: &mut Vec<u8>, node: &Node, shape: &Shape) -> Result<(), CodecError> {
    if *node == Node::Null {
        put_i32(out, -1);
        return Ok(());
    }
    let mark = out.len();
    put_i32(out, 0);
    encode(node, shape, out)?;
    let length = i32::try_from(out.len() - mark - 4)?;
    out[mark..mark + 4].copy_from_slice(&length.to_be_bytes());
    Ok(())
}

/// Appends the binary body of `node` as a value of type `shape`.
///
/// # Errors
///
/// An error when the node does not match the shape.
pub fn encode(node: &Node, shape: &Shape, out: &mut Vec<u8>) -> Result<(), CodecError> {
    match (node, shape) {
        (Node::Record(nodes), Shape::Record(fields)) if nodes.len() == fields.len() => {
            put_i32(out, i32::try_from(nodes.len())?);
            for (node, (oid, shape)) in nodes.iter().zip(fields) {
                out.extend_from_slice(&oid.to_be_bytes());
                put_length_prefixed(out, node, shape)?;
            }
        }
        (Node::Array(nodes), Shape::Array(oid, inner)) => {
            put_i32(out, i32::from(!nodes.is_empty()));
            put_i32(out, i32::from(nodes.contains(&Node::Null)));
            out.extend_from_slice(&oid.to_be_bytes());
            if !nodes.is_empty() {
                put_i32(out, i32::try_from(nodes.len())?);
                put_i32(out, 1);
                for node in nodes {
                    put_length_prefixed(out, node, inner)?;
                }
            }
        }
        (Node::Int2(value), _) => out.extend_from_slice(&value.to_be_bytes()),
        (Node::Int4(value), _) => out.extend_from_slice(&value.to_be_bytes()),
        (Node::Int8(value), _) => out.extend_from_slice(&value.to_be_bytes()),
        (Node::Text(value), _) => out.extend_from_slice(value.as_bytes()),
        (Node::Uuid(value), _) => out.extend_from_slice(value),
        _ => return Err(fail("a node does not match its declared type")),
    }
    Ok(())
}
