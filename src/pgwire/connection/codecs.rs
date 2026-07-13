use crate::executor::ColumnMeta;
use crate::types::Value;
use std::{io, str};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

const OID_BOOL: i64 = 16;
const OID_BYTEA: i64 = 17;
const OID_INT8: i64 = 20;
const OID_INT2: i64 = 21;
const OID_INT4: i64 = 23;
const OID_TEXT: i64 = 25;
const OID_JSON: i64 = 114;
const OID_FLOAT8: i64 = 701;
const OID_DATE: i64 = 1082;
const OID_TIME: i64 = 1083;
const OID_TIMESTAMP: i64 = 1114;
const OID_BPCHAR: i64 = 1042;
const OID_VARCHAR: i64 = 1043;
const OID_UUID: i64 = 2950;
const OID_UNKNOWN: i64 = 705;
const OID_VECTOR_BASE: i64 = 33_000;
const OID_ARRAY_BASE: i64 = 34_000;
const OID_ARRAY_LIMIT: i64 = 44_000;
const POSTGRES_EPOCH_JULIAN_DAY: i32 = 2_451_545;
const MICROSECONDS_PER_DAY: i64 = 86_400_000_000;

#[derive(Debug, Clone, Copy)]
enum BinaryCodecKind {
    Bool,
    Bytea,
    Int2,
    Int4,
    Int8,
    Float8,
    Text,
    Json,
    Uuid,
    Date,
    Time,
    Timestamp,
}

#[derive(Debug, Clone, Copy)]
struct BinaryCodec {
    oid: i64,
    name: &'static str,
    kind: BinaryCodecKind,
}

const BINARY_CODEC_REGISTRY: &[BinaryCodec] = &[
    BinaryCodec {
        oid: OID_BOOL,
        name: "bool",
        kind: BinaryCodecKind::Bool,
    },
    BinaryCodec {
        oid: OID_BYTEA,
        name: "bytea",
        kind: BinaryCodecKind::Bytea,
    },
    BinaryCodec {
        oid: OID_INT8,
        name: "int8",
        kind: BinaryCodecKind::Int8,
    },
    BinaryCodec {
        oid: OID_INT2,
        name: "int2",
        kind: BinaryCodecKind::Int2,
    },
    BinaryCodec {
        oid: OID_INT4,
        name: "int4",
        kind: BinaryCodecKind::Int4,
    },
    BinaryCodec {
        oid: OID_TEXT,
        name: "text",
        kind: BinaryCodecKind::Text,
    },
    BinaryCodec {
        oid: OID_JSON,
        name: "json",
        kind: BinaryCodecKind::Json,
    },
    BinaryCodec {
        oid: OID_FLOAT8,
        name: "float8",
        kind: BinaryCodecKind::Float8,
    },
    BinaryCodec {
        oid: OID_DATE,
        name: "date",
        kind: BinaryCodecKind::Date,
    },
    BinaryCodec {
        oid: OID_TIME,
        name: "time",
        kind: BinaryCodecKind::Time,
    },
    BinaryCodec {
        oid: OID_TIMESTAMP,
        name: "timestamp",
        kind: BinaryCodecKind::Timestamp,
    },
    BinaryCodec {
        oid: OID_BPCHAR,
        name: "bpchar",
        kind: BinaryCodecKind::Text,
    },
    BinaryCodec {
        oid: OID_VARCHAR,
        name: "varchar",
        kind: BinaryCodecKind::Text,
    },
    BinaryCodec {
        oid: OID_UUID,
        name: "uuid",
        kind: BinaryCodecKind::Uuid,
    },
    BinaryCodec {
        oid: OID_UNKNOWN,
        name: "unknown",
        kind: BinaryCodecKind::Text,
    },
];

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
    let codec = binary_codec_for_oid(type_oid)?;
    match codec.kind {
        BinaryCodecKind::Bool => match value {
            Value::Bool(value) => Ok(vec![u8::from(value)]),
            Value::String(value) => parse_bool_binary(&value),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Bytea => match value {
            Value::String(value) => decode_bytea(&value),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Int2 => encode_integer(value, codec.name, encode_i16_binary),
        BinaryCodecKind::Int4 => encode_integer(value, codec.name, encode_i32_binary),
        BinaryCodecKind::Int8 => encode_int8(value, codec.name),
        BinaryCodecKind::Float8 => encode_float8(value, codec.name),
        BinaryCodecKind::Text => Ok(value_to_text(value).into_bytes()),
        BinaryCodecKind::Json => match value {
            Value::Json(value) => Ok(value.to_string().into_bytes()),
            Value::String(value) => Ok(value.into_bytes()),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Uuid => match value {
            Value::String(value) => uuid::Uuid::parse_str(&value)
                .map(|value| value.into_bytes().to_vec())
                .map_err(|_| invalid_data(codec.name)),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Date => match value {
            Value::String(value) => encode_date(&value).map(|value| value.to_be_bytes().to_vec()),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Time => match value {
            Value::String(value) => encode_time(&value).map(|value| value.to_be_bytes().to_vec()),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Timestamp => match value {
            Value::String(value) => {
                encode_timestamp(&value).map(|value| value.to_be_bytes().to_vec())
            }
            _ => invalid_value(codec.name),
        },
    }
}

fn binary_codec_for_oid(type_oid: i64) -> io::Result<BinaryCodec> {
    BINARY_CODEC_REGISTRY
        .iter()
        .find(|codec| codec.oid == type_oid)
        .copied()
        .ok_or_else(|| unsupported_codec(type_oid))
}

pub(super) fn validate_result_formats(
    columns: &[ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    if !result_formats.iter().all(|format| matches!(*format, 0 | 1)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format",
        ));
    }
    if !result_formats.is_empty()
        && result_formats.len() != 1
        && result_formats.len() != columns.len()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format count",
        ));
    }
    for (index, column) in columns.iter().enumerate() {
        if result_format_for_index(result_formats, index) == 1 {
            binary_codec_for_oid(column.type_oid)?;
        }
    }
    Ok(())
}

pub(super) fn binary_to_value(parameter: &[u8], type_oid: i64) -> io::Result<Value> {
    let codec = binary_codec_for_oid(type_oid)?;
    match codec.kind {
        BinaryCodecKind::Bool => {
            fixed_bytes::<1>(parameter, codec.name).and_then(|bytes| match bytes[0] {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                _ => Err(invalid_data(codec.name)),
            })
        }
        BinaryCodecKind::Bytea => Ok(Value::String(hex_bytea(parameter))),
        BinaryCodecKind::Int2 => fixed_bytes::<2>(parameter, codec.name)
            .map(i16::from_be_bytes)
            .map(i64::from)
            .map(Value::Int64),
        BinaryCodecKind::Int4 => fixed_bytes::<4>(parameter, codec.name)
            .map(i32::from_be_bytes)
            .map(i64::from)
            .map(Value::Int64),
        BinaryCodecKind::Int8 => fixed_bytes::<8>(parameter, codec.name)
            .map(i64::from_be_bytes)
            .map(Value::Int64),
        BinaryCodecKind::Float8 => fixed_bytes::<8>(parameter, codec.name)
            .map(f64::from_be_bytes)
            .map(Value::Float64),
        BinaryCodecKind::Text => std::str::from_utf8(parameter)
            .map(|value| Value::String(value.to_string()))
            .map_err(|_| invalid_data(codec.name)),
        BinaryCodecKind::Json => {
            let value = std::str::from_utf8(parameter).map_err(|_| invalid_data(codec.name))?;
            serde_json::from_str(value)
                .map(Value::Json)
                .map_err(|_| invalid_data(codec.name))
        }
        BinaryCodecKind::Uuid => uuid::Uuid::from_slice(parameter)
            .map(|value| Value::String(value.to_string()))
            .map_err(|_| invalid_data(codec.name)),
        BinaryCodecKind::Date => decode_date(parameter).map(Value::String),
        BinaryCodecKind::Time => decode_time(parameter).map(Value::String),
        BinaryCodecKind::Timestamp => decode_timestamp(parameter).map(Value::String),
    }
}

fn result_format_for_index(formats: &[i16], index: usize) -> i16 {
    match formats.len() {
        0 => 0,
        1 => formats[0],
        _ => formats[index],
    }
}

fn encode_integer(
    value: Value,
    type_name: &str,
    encode: fn(i64) -> io::Result<Vec<u8>>,
) -> io::Result<Vec<u8>> {
    match value {
        Value::Int64(value) => encode(value),
        Value::String(value) => parse_i64_binary(&value).and_then(encode),
        _ => invalid_value(type_name),
    }
}

fn encode_int8(value: Value, type_name: &str) -> io::Result<Vec<u8>> {
    match value {
        Value::Int64(value) => Ok(value.to_be_bytes().to_vec()),
        Value::Float64(value) => encode_f64_as_i64_binary(value),
        Value::String(value) => parse_i64_binary(&value).map(|value| value.to_be_bytes().to_vec()),
        _ => invalid_value(type_name),
    }
}

fn encode_float8(value: Value, type_name: &str) -> io::Result<Vec<u8>> {
    match value {
        Value::Float64(value) => Ok(value.to_be_bytes().to_vec()),
        Value::Int64(value) => encode_i64_as_f64_binary(value),
        Value::String(value) => parse_f64_binary(&value).map(|value| value.to_be_bytes().to_vec()),
        _ => invalid_value(type_name),
    }
}

fn fixed_bytes<const N: usize>(value: &[u8], type_name: &str) -> io::Result<[u8; N]> {
    value.try_into().map_err(|_| invalid_data(type_name))
}

fn unsupported_codec(type_oid: i64) -> io::Error {
    let family = if (OID_VECTOR_BASE..OID_ARRAY_BASE).contains(&type_oid) {
        "vector"
    } else if (OID_ARRAY_BASE..OID_ARRAY_LIMIT).contains(&type_oid) {
        "array"
    } else {
        "type"
    };
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("binary format is not supported for {family} OID {type_oid}"),
    )
}

fn invalid_data(type_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid binary {type_name} value"),
    )
}

fn invalid_value(type_name: &str) -> io::Result<Vec<u8>> {
    Err(invalid_data(type_name))
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

fn encode_date(value: &str) -> io::Result<i32> {
    let date = parse_date(value)?;
    date.to_julian_day()
        .checked_sub(POSTGRES_EPOCH_JULIAN_DAY)
        .ok_or_else(|| invalid_data("date"))
}

fn encode_time(value: &str) -> io::Result<i64> {
    let time = parse_time(value)?;
    let micros = i64::from(time.hour()) * 3_600_000_000
        + i64::from(time.minute()) * 60_000_000
        + i64::from(time.second()) * 1_000_000
        + i64::from(time.microsecond());
    Ok(micros)
}

fn encode_timestamp(value: &str) -> io::Result<i64> {
    let datetime = parse_timestamp(value)?;
    let days = i64::from(
        datetime
            .date()
            .to_julian_day()
            .checked_sub(POSTGRES_EPOCH_JULIAN_DAY)
            .ok_or_else(|| invalid_data("timestamp"))?,
    );
    let time = encode_time(&format_time(datetime.time()))?;
    days.checked_mul(MICROSECONDS_PER_DAY)
        .and_then(|days| days.checked_add(time))
        .ok_or_else(|| invalid_data("timestamp"))
}

fn decode_date(bytes: &[u8]) -> io::Result<String> {
    let days = i32::from_be_bytes(fixed_bytes(bytes, "date")?);
    let julian_day = POSTGRES_EPOCH_JULIAN_DAY
        .checked_add(days)
        .ok_or_else(|| invalid_data("date"))?;
    let date = Date::from_julian_day(julian_day).map_err(|_| invalid_data("date"))?;
    Ok(format_date(date))
}

fn decode_time(bytes: &[u8]) -> io::Result<String> {
    let micros = i64::from_be_bytes(fixed_bytes(bytes, "time")?);
    if !(0..MICROSECONDS_PER_DAY).contains(&micros) {
        return Err(invalid_data("time"));
    }
    let time = time_from_microseconds(micros)?;
    Ok(format_time(time))
}

fn decode_timestamp(bytes: &[u8]) -> io::Result<String> {
    let micros = i64::from_be_bytes(fixed_bytes(bytes, "timestamp")?);
    let days = micros.div_euclid(MICROSECONDS_PER_DAY);
    let remainder = micros.rem_euclid(MICROSECONDS_PER_DAY);
    let days = i32::try_from(days).map_err(|_| invalid_data("timestamp"))?;
    let julian_day = POSTGRES_EPOCH_JULIAN_DAY
        .checked_add(days)
        .ok_or_else(|| invalid_data("timestamp"))?;
    let date = Date::from_julian_day(julian_day).map_err(|_| invalid_data("timestamp"))?;
    let time = time_from_microseconds(remainder)?;
    let datetime = PrimitiveDateTime::new(date, time).assume_offset(UtcOffset::UTC);
    datetime
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| invalid_data("timestamp"))
}

fn parse_date(value: &str) -> io::Result<Date> {
    let mut parts = value.split('-');
    let year = parts
        .next()
        .ok_or_else(|| invalid_data("date"))?
        .parse::<i32>()
        .map_err(|_| invalid_data("date"))?;
    let month = parts
        .next()
        .ok_or_else(|| invalid_data("date"))?
        .parse::<u8>()
        .map_err(|_| invalid_data("date"))?;
    let day = parts
        .next()
        .ok_or_else(|| invalid_data("date"))?
        .parse::<u8>()
        .map_err(|_| invalid_data("date"))?;
    if parts.next().is_some() {
        return Err(invalid_data("date"));
    }
    let month = Month::try_from(month).map_err(|_| invalid_data("date"))?;
    Date::from_calendar_date(year, month, day).map_err(|_| invalid_data("date"))
}

fn parse_time(value: &str) -> io::Result<Time> {
    let (clock, fraction) = value.split_once('.').map_or((value, ""), |parts| parts);
    let mut parts = clock.split(':');
    let hour = parts
        .next()
        .ok_or_else(|| invalid_data("time"))?
        .parse::<u8>()
        .map_err(|_| invalid_data("time"))?;
    let minute = parts
        .next()
        .ok_or_else(|| invalid_data("time"))?
        .parse::<u8>()
        .map_err(|_| invalid_data("time"))?;
    let second = parts
        .next()
        .ok_or_else(|| invalid_data("time"))?
        .parse::<u8>()
        .map_err(|_| invalid_data("time"))?;
    if parts.next().is_some()
        || fraction.len() > 6
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid_data("time"));
    }
    let micros = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u32>()
            .map_err(|_| invalid_data("time"))?
            .saturating_mul(10_u32.pow(6 - u32::try_from(fraction.len()).unwrap_or(6)))
    };
    Time::from_hms_micro(hour, minute, second, micros).map_err(|_| invalid_data("time"))
}

fn parse_timestamp(value: &str) -> io::Result<PrimitiveDateTime> {
    if let Ok(datetime) =
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
    {
        let datetime = datetime.to_offset(UtcOffset::UTC);
        return Ok(PrimitiveDateTime::new(datetime.date(), datetime.time()));
    }

    let normalized = value.replace(' ', "T");
    let (date, time) = normalized
        .split_once('T')
        .ok_or_else(|| invalid_data("timestamp"))?;
    Ok(PrimitiveDateTime::new(parse_date(date)?, parse_time(time)?))
}

fn time_from_microseconds(micros: i64) -> io::Result<Time> {
    let hour = u8::try_from(micros / 3_600_000_000).map_err(|_| invalid_data("time"))?;
    let remainder = micros.rem_euclid(3_600_000_000);
    let minute = u8::try_from(remainder / 60_000_000).map_err(|_| invalid_data("time"))?;
    let remainder = remainder.rem_euclid(60_000_000);
    let second = u8::try_from(remainder / 1_000_000).map_err(|_| invalid_data("time"))?;
    let microsecond =
        u32::try_from(remainder.rem_euclid(1_000_000)).map_err(|_| invalid_data("time"))?;
    Time::from_hms_micro(hour, minute, second, microsecond).map_err(|_| invalid_data("time"))
}

fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn format_time(time: Time) -> String {
    if time.microsecond() == 0 {
        format!(
            "{:02}:{:02}:{:02}",
            time.hour(),
            time.minute(),
            time.second()
        )
    } else {
        format!(
            "{:02}:{:02}:{:02}.{:06}",
            time.hour(),
            time.minute(),
            time.second(),
            time.microsecond()
        )
    }
}

fn hex_bytea(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(2 + bytes.len().saturating_mul(2));
    out.push_str("\\x");
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
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
