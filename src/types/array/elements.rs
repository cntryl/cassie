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
                DataType::Float => token
                    .get()
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
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
        DataType::Boolean => value
            .as_bool()
            .map(JsonValue::Bool)
            .ok_or_else(|| "boolean array element expects bool".to_string()),
        DataType::Float => value
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map(JsonValue::Number)
            .ok_or_else(|| "float array element expects a finite f64 number".to_string()),
        DataType::Text
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
        DataType::Json | DataType::Vector(_) | DataType::Array(_) | DataType::Null => Ok(value),
    }
}

fn normalize_string(text: String, data_type: &DataType) -> Result<String, String> {
    match data_type {
        DataType::Char {
            length: Some(length),
        } => {
            let text = crate::types::char_text::canonical_char_text(&text).to_string();
            validate_character_bound(&text, *length)?;
            Ok(text)
        }
        DataType::Varchar {
            length: Some(length),
        } => {
            validate_character_bound(&text, *length)?;
            Ok(text)
        }
        DataType::Uuid => uuid::Uuid::parse_str(text.trim())
            .map(|uuid| uuid.to_string())
            .map_err(|_| "invalid UUID array element".to_string()),
        DataType::Bytea => {
            crate::midge::row_blob::canonical_bytea_text(&text).map_err(|error| error.to_string())
        }
        DataType::Date => crate::types::temporal::canonical_date(text.trim()),
        DataType::Time => crate::types::temporal::canonical_time(text.trim()),
        DataType::Timestamp => crate::types::temporal::canonical_timestamp(text.trim()),
        _ => Ok(text),
    }
}

fn validate_character_bound(text: &str, length: u32) -> Result<(), String> {
    let maximum = usize::try_from(length)
        .map_err(|_| "array character bound exceeds the platform range".to_string())?;
    if text.chars().count() > maximum {
        return Err("array character element exceeds its declared bound".to_string());
    }
    Ok(())
}

fn normalize_integer(value: &JsonValue, minimum: i64, maximum: i64) -> Result<JsonValue, String> {
    value
        .as_i64()
        .filter(|number| (minimum..=maximum).contains(number))
        .map(JsonValue::from)
        .ok_or_else(|| "integer array element is outside its declared domain".to_string())
}
