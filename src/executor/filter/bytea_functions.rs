//! Fixed PostgreSQL BYTEA routines; the row carrier remains canonical hex.
use super::{QueryError, Value};
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

#[path = "bytea_codecs.rs"]
mod codecs;
#[path = "bytea_work.rs"]
mod work;
use work::Work;

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    let metadata = crate::sql::functions::function(name)?;
    if !crate::sql::functions::is_bytea_function(metadata.id) {
        return None;
    }
    Some(run(name, args, controls))
}

fn run(
    name: &str,
    args: &[Value],
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let work = Work::admit(name, args, controls)?;
    let text = |index| match args.get(index) {
        Some(Value::String(value)) => Ok(value.as_str()),
        _ => Err(QueryError::General(
            "invalid BYTEA function argument".into(),
        )),
    };
    let number = |index| match args.get(index) {
        Some(Value::Int64(value)) => Ok(*value),
        _ => Err(QueryError::General("invalid BYTEA integer argument".into())),
    };
    if name == "decode" {
        return codecs::decode(text(0)?, text(1)?, &work)
            .and_then(|bytes| carrier(&bytes, &work).map(Value::String));
    }
    let mut bytes = codecs::hex_bytes(
        text(0)?
            .strip_prefix("\\x")
            .ok_or_else(|| QueryError::General("invalid canonical BYTEA carrier".into()))?,
        &work,
    )?;
    match name {
        "encode" => codecs::encode(&bytes, text(1)?, &work).map(Value::String),
        "bit_count" => {
            let mut count = 0_i64;
            for chunk in bytes.chunks(4096) {
                work.check()?;
                count += chunk
                    .iter()
                    .map(|byte| i64::from(byte.count_ones()))
                    .sum::<i64>();
            }
            Ok(Value::Int64(count))
        }
        "get_byte" | "set_byte" | "get_bit" | "set_bit" => {
            let index = number(1)?;
            let bits = matches!(name, "get_bit" | "set_bit");
            let length = bytes
                .len()
                .checked_mul(if bits { 8 } else { 1 })
                .ok_or_else(|| {
                    QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                        "BYTEA bit length overflow".into(),
                    ))
                })?;
            let position = usize::try_from(index)
                .ok()
                .filter(|index| *index < length)
                .ok_or_else(|| {
                    QueryError::Cassie(crate::app::CassieError::ByteaIndex {
                        index,
                        maximum: i64::try_from(length).unwrap_or(i64::MAX) - 1,
                    })
                })?;
            if name == "get_byte" {
                return Ok(Value::Int64(i64::from(bytes[position])));
            }
            if name == "set_byte" {
                bytes[position] = number(2)?.to_le_bytes()[0];
            }
            if bits {
                let mask = 1 << (position % 8);
                let byte = &mut bytes[position / 8];
                if name == "get_bit" {
                    return Ok(Value::Int64(i64::from(u8::from(*byte & mask != 0))));
                }
                match number(2)? {
                    0 => *byte &= !mask,
                    1 => *byte |= mask,
                    _ => return Err(invalid("new bit must be 0 or 1".into())),
                }
            }
            Ok(Value::String(carrier(&bytes, &work)?))
        }
        "sha224" => Ok(Value::String(hash::<Sha224>(&bytes, &work)?)),
        "sha256" => Ok(Value::String(hash::<Sha256>(&bytes, &work)?)),
        "sha384" => Ok(Value::String(hash::<Sha384>(&bytes, &work)?)),
        "sha512" => Ok(Value::String(hash::<Sha512>(&bytes, &work)?)),
        _ => unreachable!("fixed BYTEA metadata"),
    }
}

fn invalid(message: String) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::InvalidParameterValue(message))
}

fn carrier(bytes: &[u8], work: &Work<'_>) -> Result<String, QueryError> {
    let mut output = String::with_capacity(2 + bytes.len() * 2);
    output.push_str("\\x");
    crate::types::bytea_input::append_hex(&mut output, bytes, || work.check())?;
    Ok(output)
}

fn hash<D: Digest>(bytes: &[u8], work: &Work<'_>) -> Result<String, QueryError> {
    let mut hash = D::new();
    for chunk in bytes.chunks(4096) {
        work.check()?;
        hash.update(chunk);
    }
    carrier(&hash.finalize(), work)
}

#[cfg(test)]
#[path = "bytea_resource_tests.rs"]
mod resource_tests;
