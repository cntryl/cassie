use super::{Value, io, str};

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
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Json(v) => v.to_string(),
    }
}

pub(super) fn value_to_binary(value: Value, type_oid: i64) -> io::Result<Vec<u8>> {
    match type_oid {
        16 => match value {
            Value::Bool(v) => Ok(vec![u8::from(v)]),
            Value::String(v) => parse_bool_binary(&v),
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
            Value::Int64(v) => encode_i16_binary(v),
            Value::String(v) => parse_i64_binary(&v).and_then(encode_i16_binary),
            other => Ok(value_to_text(other).into_bytes()),
        },
        20 => match value {
            Value::Int64(v) => Ok(v.to_be_bytes().to_vec()),
            Value::Float64(v) => encode_f64_as_i64_binary(v),
            Value::String(v) => parse_i64_binary(&v).map(|value| value.to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        23 => match value {
            Value::Int64(v) => encode_i32_binary(v),
            Value::String(v) => parse_i64_binary(&v).and_then(encode_i32_binary),
            other => Ok(value_to_text(other).into_bytes()),
        },
        701 => match value {
            Value::Float64(v) => Ok(v.to_be_bytes().to_vec()),
            Value::Int64(v) => encode_i64_as_f64_binary(v),
            Value::String(v) => parse_f64_binary(&v).map(|value| value.to_be_bytes().to_vec()),
            other => Ok(value_to_text(other).into_bytes()),
        },
        _ => Ok(value_to_text(value).into_bytes()),
    }
}

fn encode_i16_binary(value: i64) -> io::Result<Vec<u8>> {
    i16::try_from(value)
        .map(|value| value.to_be_bytes().to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "int2 out of range"))
}

fn encode_i32_binary(value: i64) -> io::Result<Vec<u8>> {
    i32::try_from(value)
        .map(|value| value.to_be_bytes().to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "int4 out of range"))
}

fn encode_f64_as_i64_binary(value: f64) -> io::Result<Vec<u8>> {
    let integer = parse_f64_to_i64(value)?;
    Ok(integer.to_be_bytes().to_vec())
}

fn encode_i64_as_f64_binary(value: i64) -> io::Result<Vec<u8>> {
    parse_i64_to_f64(value).map(|value| value.to_be_bytes().to_vec())
}

fn parse_bool_binary(value: &str) -> io::Result<Vec<u8>> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "t" | "1" => Ok(vec![1]),
        "false" | "f" | "0" => Ok(vec![0]),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode bool value to binary",
        )),
    }
}

fn parse_i64_binary(value: &str) -> io::Result<i64> {
    value.parse::<i64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode integer value to binary",
        )
    })
}

fn parse_f64_binary(value: &str) -> io::Result<f64> {
    value.parse::<f64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode float value to binary",
        )
    })
}

fn parse_f64_to_i64(value: f64) -> io::Result<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode non-integral float as int8",
        ));
    }

    format!("{value:.0}").parse::<i64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode float value to int8",
        )
    })
}

fn parse_i64_to_f64(value: i64) -> io::Result<f64> {
    value.to_string().parse::<f64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode int8 value to float8",
        )
    })
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
