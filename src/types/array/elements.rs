use serde_json::Value as JsonValue;

use crate::types::DataType;

pub(super) fn parse_json_numeric_array(
    input: &str,
    data_type: &DataType,
) -> Result<JsonValue, String> {
    // Borrow original tokens after complete JSON grammar validation, so the
    // document decoder cannot round FLOAT or reclassify lexical integer -0.
    let tokens: Vec<&serde_json::value::RawValue> =
        serde_json::from_str(input).map_err(|error| format!("invalid JSON array: {error}"))?;
    tokens
        .into_iter()
        .map(|token| {
            if token.get() == "null" {
                return Ok(JsonValue::Null);
            }
            let value = match data_type {
                DataType::Float => serde_json::Number::from_f64(
                    crate::types::float_input::parse(token.get())
                        .map_err(|error| error.to_string())?,
                )
                .map(JsonValue::Number)
                .ok_or_else(|| "float array element expects a finite f64 number".to_string())?,
                DataType::SmallInt | DataType::Int | DataType::BigInt => token
                    .get()
                    .parse::<i64>()
                    .map(JsonValue::from)
                    .map_err(|_| "integer array element expects an exact i64 token".to_string())?,
                _ => return Err("numeric array element family required".to_string()),
            };
            normalize_element(value, data_type)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(JsonValue::Array)
}

pub(super) fn parse_json_document(input: &str) -> Result<JsonValue, String> {
    let parsed = crate::runtime::accounted::json_arena::parse_value_with_optional_controls(
        input.as_bytes(),
        None,
    )
    .map_err(|error| match error {
        crate::runtime::accounted::json_arena::ValueParseError::Invalid => {
            format!(
                "invalid JSON document: {}",
                crate::runtime::accounted::json_arena::syntax_error(input.as_bytes())
            )
        }
        crate::runtime::accounted::json_arena::ValueParseError::Controlled(error) => {
            error.to_string()
        }
    })?;
    Ok(parsed.get().clone())
}

pub(super) fn normalize_element(
    value: JsonValue,
    data_type: &DataType,
) -> Result<JsonValue, String> {
    if value.is_null() {
        return Ok(value);
    }
    match data_type {
        DataType::SmallInt => normalize_integer(&value, i64::from(i16::MIN), i64::from(i16::MAX)),
        DataType::Int => normalize_integer(&value, i64::from(i32::MIN), i64::from(i32::MAX)),
        DataType::BigInt => normalize_integer(&value, i64::MIN, i64::MAX),
        DataType::ObjectId(_) => normalize_integer(&value, 0, crate::types::object_id::MAX_OID),
        DataType::Extended(kind) => kind
            .from_json(&value)
            .map(|typed| typed.to_json())
            .map_err(|error| error.to_string()),
        DataType::Boolean => value
            .as_bool()
            .map(JsonValue::Bool)
            .ok_or_else(|| "boolean array element expects bool".to_string()),
        DataType::Float => value
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map(JsonValue::Number)
            .ok_or_else(|| "float array element expects a finite f64 number".to_string()),
        DataType::Name
        | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::Uuid
        | DataType::Bytea
        | DataType::Date
        | DataType::Time
        | DataType::Timestamp => {
            let JsonValue::String(text) = value else {
                return Err("string-backed array element expects string".to_string());
            };
            normalize_string(text, data_type).map(JsonValue::String)
        }
        // These are document/storage families, not the selected scalar-array
        // wire family matrix. Keep their existing element policy.
        DataType::Json
        | DataType::Jsonb
        | DataType::JsonPath
        | DataType::Vector(_)
        | DataType::Array(_)
        | DataType::Null => Ok(value),
    }
}

fn normalize_string(text: String, data_type: &DataType) -> Result<String, String> {
    match data_type {
        DataType::Name => Ok(crate::types::name::parse_text(&text)),
        DataType::Char { length: Some(_) } | DataType::Varchar { length: Some(_) } => {
            character_input(&text, data_type)
                .expect("character input")
                .map_err(|error| error.to_string())
        }
        DataType::Uuid => uuid::Uuid::parse_str(text.trim())
            .map(|uuid| uuid.to_string())
            .map_err(|_| "invalid UUID array element".to_string()),
        DataType::Bytea => {
            crate::midge::row_blob::canonical_bytea_text(&text).map_err(|error| error.to_string())
        }
        DataType::Date | DataType::Time | DataType::Timestamp => {
            use crate::types::extended::ExtendedType;
            let kind = match data_type {
                DataType::Date => ExtendedType::Date,
                DataType::Time => ExtendedType::Time { precision: None },
                _ => ExtendedType::Timestamp { precision: None },
            };
            let value = kind
                .parse_text(text.trim())
                .map_err(|error| error.to_string())?;
            Ok(
                crate::types::extended::temporal_legacy_text(&value)
                    .unwrap_or_else(|| value.text()),
            )
        }
        _ => Ok(text),
    }
}

pub(super) fn character_input(
    text: &str,
    data_type: &DataType,
) -> Option<Result<String, crate::types::extended::TypeInputError>> {
    let (width, padded) = match data_type {
        DataType::Char { length } => (*length, true),
        DataType::Varchar { length } => (*length, false),
        _ => return None,
    };
    Some(
        crate::types::char_text::conversion_plan(text, width, padded, false).map(|plan| {
            let text = &text[..plan.prefix_bytes];
            // This parser also feeds the existing canonical stored CHAR carrier.
            // Query casts materialize padding only after their own admission.
            if padded && width.is_some() {
                crate::types::char_text::canonical_char_text(text).to_owned()
            } else {
                text.to_owned()
            }
        }),
    )
}

fn normalize_integer(value: &JsonValue, minimum: i64, maximum: i64) -> Result<JsonValue, String> {
    value
        .as_i64()
        .filter(|number| (minimum..=maximum).contains(number))
        .map(JsonValue::from)
        .ok_or_else(|| "integer array element is outside its declared domain".to_string())
}
