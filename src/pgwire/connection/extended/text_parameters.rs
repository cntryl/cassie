//! Text-format (format code 0) bind parameter decoding.
//!
//! A bind parameter must decode to the same `Value` whether the client sent
//! it in text or binary format, so every declared OID with a binary codec
//! canonicalizes its text form here the way `binary_to_value` does, and
//! accepts exactly the spellings the PostgreSQL input function accepts.

use super::ExtendedQueryError;
use crate::types::builtin::{builtin_for_oid, BuiltinKind};
use crate::types::extended::ExtendedType;
use crate::types::{DataType, Value};

// Text-only historical numeric adapters; their complete types/codecs remain staged.
const OID_FLOAT4: i32 = 700;
const OID_NUMERIC: i32 = 1700;

/// Temporal text parameters decode natively; zone-free `timestamptz`
/// text stays text for the statement's `TimeZone`.
fn temporal_parameter(text: &str, kind: ExtendedType) -> Result<Value, ExtendedQueryError> {
    match crate::types::extended::temporal_parameter_from_text(kind, text) {
        Some(Ok(value)) => Ok(Value::Typed(value)),
        Some(Err(error)) => Err(ExtendedQueryError::cassie(
            &crate::app::CassieError::TypeInput(error),
        )),
        None => Ok(Value::String(text.to_string())),
    }
}

pub(super) fn decode_text_parameter(
    parameter: &[u8],
    oid: i32,
) -> Result<Value, ExtendedQueryError> {
    let text = encoding::decode(parameter)?;
    let kind = u32::try_from(oid)
        .ok()
        .and_then(builtin_for_oid)
        .map(|metadata| metadata.kind);
    match kind {
        Some(BuiltinKind::Bool) => parse_bool(text).map(Value::Bool),
        Some(BuiltinKind::Int2) => parse_integer(text, crate::types::IntegerWidth::I16),
        Some(BuiltinKind::Int4) => parse_integer(text, crate::types::IntegerWidth::I32),
        Some(BuiltinKind::Int8) => parse_integer(text, crate::types::IntegerWidth::I64),
        Some(BuiltinKind::Float8) => parse_float(text).map(Value::Float64),
        Some(BuiltinKind::Name) => Ok(Value::String(crate::types::name::parse_text(text))),
        Some(BuiltinKind::Extended(kind)) if kind.binds_parameters() => {
            kind.parse_text(text).map(Value::Typed).map_err(|error| {
                ExtendedQueryError::cassie(&crate::app::CassieError::TypeInput(error))
            })
        }
        _ if oid == OID_NUMERIC => parse_numeric(text),
        _ if oid == OID_FLOAT4 => parse_float(text).map(Value::Float64),
        Some(BuiltinKind::Jsonb) => {
            crate::types::jsonb::canonical(text).map_err(|error| {
                ExtendedQueryError::cassie(&crate::app::CassieError::TypeInput(error))
            })?;
            serde_json::from_str(text)
                .map(Value::Json)
                .map_err(|_| ExtendedQueryError::protocol("invalid JSONB bind parameter"))
        }
        Some(BuiltinKind::Json) => serde_json::from_str(text)
            .map(Value::Json)
            .map_err(|_| ExtendedQueryError::protocol("invalid JSON bind parameter")),
        Some(BuiltinKind::JsonPath) => sql_json_path::JsonPath::new(text)
            .map(|path| Value::String(path.to_string()))
            .map_err(|error| {
                ExtendedQueryError::cassie(&crate::app::CassieError::Parse(error.to_string()))
            }),
        Some(BuiltinKind::Date) => temporal_parameter(text, ExtendedType::Date),
        Some(BuiltinKind::Time) => temporal_parameter(text, ExtendedType::Time { precision: None }),
        Some(BuiltinKind::Timestamp) => {
            temporal_parameter(text, ExtendedType::Timestamp { precision: None })
        }
        Some(BuiltinKind::Extended(
            kind @ (ExtendedType::TimeTz { .. }
            | ExtendedType::TimestampTz { .. }
            | ExtendedType::Interval { .. }),
        )) => temporal_parameter(text, kind),
        Some(BuiltinKind::Uuid) => uuid::Uuid::parse_str(text.trim())
            .map(|uuid| Value::String(uuid.to_string()))
            .map_err(|_| ExtendedQueryError::invalid_input_syntax("uuid", text)),
        Some(BuiltinKind::Extended(kind)) if kind.binds_parameters() => kind
            .parse_text(text)
            .map(Value::Typed)
            .map_err(|error| ExtendedQueryError::cassie(&error.into())),
        Some(BuiltinKind::Bytea) => crate::types::bytea_input::canonical(text)
            .map(Value::String)
            .map_err(|error| ExtendedQueryError::cassie(&error.into())),
        _ => decode_array_or_text(text, oid),
    }
}

fn decode_array_or_text(text: &str, oid: i32) -> Result<Value, ExtendedQueryError> {
    match crate::sql::binder::parameter_data_type_for_oid(oid) {
        Some(DataType::Array(element_type)) => {
            crate::types::array::parse_text_array_with_dimensions(text, &element_type)
                .map_err(|error| match error {
                    crate::types::array::TextArrayError::Invalid(_) => {
                        ExtendedQueryError::protocol("invalid text array parameter")
                    }
                    crate::types::array::TextArrayError::UnsupportedJsonDocumentNull => {
                        ExtendedQueryError::unsupported(error.to_string())
                    }
                })
                .and_then(|mut parsed| {
                    let json_elements =
                        matches!(element_type.as_ref(), DataType::Json | DataType::Jsonb);
                    // Bracket-form JSON array parameters use JSON null
                    // slots as SQL NULLs. PostgreSQL brace-form array
                    // input keeps a quoted JSON document null distinct.
                    if json_elements && text.trim_start().starts_with('[') {
                        for element in &mut parsed.flat_elements {
                            if matches!(
                                element,
                                crate::types::array::ParsedTextArrayElement::Value(value)
                                    if value.is_null()
                            ) {
                                *element = crate::types::array::ParsedTextArrayElement::SqlNull;
                            }
                        }
                    }
                    if json_elements
                        && parsed.flat_elements.iter().any(|element| {
                            matches!(
                                element,
                                crate::types::array::ParsedTextArrayElement::Value(value)
                                    if value.is_null()
                            )
                        })
                    {
                        return Err(ExtendedQueryError::unsupported(
                            crate::types::array::TextArrayError::UnsupportedJsonDocumentNull
                                .to_string(),
                        ));
                    }
                    parsed
                        .into_sql_array(&element_type)
                        .map(Value::Array)
                        .map_err(|_| ExtendedQueryError::protocol("invalid text array parameter"))
                })
        }
        _ => Ok(Value::String(text.to_string())),
    }
}

/// Accepts the spellings PostgreSQL's `boolin` accepts: any unique prefix of
/// `true`/`false`/`yes`/`no`, `on`/`off`, and `1`/`0`, case-insensitively and
/// with surrounding whitespace ignored.
pub(super) fn parse_bool(text: &str) -> Result<bool, ExtendedQueryError> {
    crate::types::boolean::parse_text(text)
        .ok_or_else(|| ExtendedQueryError::invalid_input_syntax("boolean", text))
}

fn parse_integer(
    text: &str,
    width: crate::types::IntegerWidth,
) -> Result<Value, ExtendedQueryError> {
    use crate::types::integer::{self, IntegerInputError};
    let name = match width {
        crate::types::IntegerWidth::I16 => "smallint",
        crate::types::IntegerWidth::I32 => "integer",
        crate::types::IntegerWidth::I64 => "bigint",
    };
    integer::parse(text, width)
        .map(Value::Int64)
        .map_err(|error| match error {
            IntegerInputError::Syntax => ExtendedQueryError::invalid_input_syntax(name, text),
            IntegerInputError::Range => ExtendedQueryError::numeric_value_out_of_range(name, text),
        })
}

fn parse_float(text: &str) -> Result<f64, ExtendedQueryError> {
    crate::types::float_input::parse(text).map_err(|error| match error {
        crate::types::FloatInputError::Syntax => {
            ExtendedQueryError::invalid_input_syntax("double precision", text)
        }
        crate::types::FloatInputError::Range => {
            ExtendedQueryError::numeric_value_out_of_range("double precision", text)
        }
    })
}

/// NUMERIC has no exact storage type, so integral text keeps full `i64`
/// precision and any other valid number decodes as a float.
fn parse_numeric(text: &str) -> Result<Value, ExtendedQueryError> {
    let text = text.trim();
    if let Ok(value) = text.parse::<i64>() {
        return Ok(Value::Int64(value));
    }
    text.parse::<f64>()
        .map(Value::Float64)
        .map_err(|_| ExtendedQueryError::protocol("invalid numeric bind parameter"))
}

#[path = "text_encoding.rs"]
mod encoding;
