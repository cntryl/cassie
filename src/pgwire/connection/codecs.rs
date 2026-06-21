use super::*;

pub(super) fn value_to_text(value: Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        Value::Float64(v) => v.to_string(),
        Value::String(v) => v,
        Value::Vector(v) => format!(
            "[{}]",
            v.values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Json(v) => v.to_string(),
    }
}

pub(super) fn value_to_binary(value: Value, type_oid: i64) -> io::Result<Vec<u8>> {
    match type_oid {
        16 => match value {
            Value::Bool(v) => Ok(vec![if v { 1 } else { 0 }]),
            other => Ok(value_to_text(other).into_bytes()),
        },
        17 => match value {
            Value::String(v) => {
                let bytes = decode_bytea(&v).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "cannot encode BYTEA value to binary",
                    )
                })?;
                Ok(bytes)
            }
            other => Ok(value_to_text(other).into_bytes()),
        },
        21 => match value {
            Value::Int64(v) => Ok((v as i16).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        20 => match value {
            Value::Int64(v) => Ok(v.to_be_bytes().to_vec()),
            Value::Float64(v) => Ok((v as i64).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        23 => match value {
            Value::Int64(v) => Ok((v as i32).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        701 => match value {
            Value::Float64(v) => Ok(v.to_be_bytes().to_vec()),
            Value::Int64(v) => Ok((v as f64).to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        25 | 1042 | 1043 | 705 | 114 => Ok(value_to_text(value).into_bytes()),
        _ => Ok(value_to_text(value).into_bytes()),
    }
}

pub(super) fn decode_bytea(value: &str) -> io::Result<Vec<u8>> {
    if !value.starts_with("\\x") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bytea value must be hex format '\\x...'",
        ));
    }
    if value.len() == 2 {
        return Ok(Vec::new());
    }
    if (value.len() - 2).rem_euclid(2) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bytea value must have an even number of hex digits",
        ));
    }

    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity((value.len() - 2) / 2);
    let mut index = 2;
    while index < value.len() {
        let high = decode_hex_digit(bytes[index]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "bytea value must be hexadecimal",
            )
        })?;
        let low = decode_hex_digit(bytes[index + 1]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "bytea value must be hexadecimal",
            )
        })?;
        out.push((high << 4) | low);
        index += 2;
    }

    Ok(out)
}

pub(super) fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
