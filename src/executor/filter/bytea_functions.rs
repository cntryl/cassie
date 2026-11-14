//! Fixed PostgreSQL BYTEA routines; the row carrier remains canonical hex.
use super::{QueryError, Value};
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

#[path = "bytea_codecs.rs"]
mod codecs;
#[path = "bytea_work.rs"]
mod work;
use work::Work;

pub(super) fn evaluate<R: super::RowAccess + ?Sized>(
    name: &str,
    function: &super::FunctionCall,
    row: &R,
    args: &[Value],
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    if super::bytea_slices::is_overloaded(name)
        && function
            .args
            .first()
            .is_some_and(|argument| expression_is_bytea(argument, row))
    {
        return Some(super::bytea_slices::evaluate(name, args));
    }
    let metadata = crate::sql::functions::function(name)?;
    if !crate::sql::functions::is_bytea_function(metadata.id) {
        return None;
    }
    if metadata.id == crate::sql::functions::FunctionId::PgcryptoDigest {
        return Some(run_digest(function, row, args, controls));
    }
    Some(run(name, args, controls))
}

fn run_digest<R: super::RowAccess + ?Sized>(
    function: &super::FunctionCall,
    row: &R,
    args: &[Value],
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let bytea_input = function
        .args
        .first()
        .is_some_and(|expression| expression_is_bytea(expression, row));
    let work = Work::admit(
        if bytea_input {
            "digest_bytea"
        } else {
            "digest_text"
        },
        args,
        controls,
    )?;
    let Some(Value::String(algorithm)) = args.get(1) else {
        return Err(QueryError::General(
            "invalid pgcrypto digest algorithm argument".into(),
        ));
    };
    if !algorithm.eq_ignore_ascii_case("sha256") {
        let algorithm = algorithm.to_ascii_lowercase();
        if [
            "md5",
            "sha1",
            "sha224",
            "sha384",
            "sha512",
            "sha3-224",
            "sha3-256",
            "sha3-384",
            "sha3-512",
            "ripemd160",
            "rmd160",
            "blake2b",
            "blake2s",
        ]
        .contains(&algorithm.as_str())
        {
            return Err(QueryError::Cassie(crate::app::CassieError::Unsupported(
                format!("pgcrypto digest algorithm '{algorithm}' is outside the selected profile"),
            )));
        }
        return Err(invalid(format!("unknown digest algorithm '{algorithm}'")));
    }
    let input = match (args.first(), bytea_input) {
        (Some(Value::String(value)), true) => {
            let digits = value
                .strip_prefix("\\x")
                .ok_or_else(|| QueryError::General("invalid canonical BYTEA carrier".into()))?;
            codecs::hex_bytes(digits, &work)?
        }
        (Some(Value::String(value)), false) => value.as_bytes().to_vec(),
        _ => return Err(QueryError::General("invalid pgcrypto digest input".into())),
    };
    Ok(Value::String(hash::<Sha256>(&input, &work)?))
}

fn expression_is_bytea<R: super::RowAccess + ?Sized>(expression: &super::Expr, row: &R) -> bool {
    match expression {
        super::Expr::Cast { data_type, .. } => *data_type == crate::types::DataType::Bytea,
        super::Expr::Column(name) => row.column_type(name) == Some(&crate::types::DataType::Bytea),
        super::Expr::Function(function) => crate::sql::functions::function(&function.name)
            .is_some_and(|metadata| {
                metadata.return_type == crate::sql::functions::FunctionReturnType::Bytea
            }),
        super::Expr::Case {
            branches,
            else_expr,
            ..
        } => {
            branches
                .iter()
                .any(|(_, value)| expression_is_bytea(value, row))
                || else_expr
                    .as_deref()
                    .is_some_and(|value| expression_is_bytea(value, row))
        }
        _ => false,
    }
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
