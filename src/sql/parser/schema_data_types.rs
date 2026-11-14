//! Column and cast type names: SQL-standard `ARRAY` suffixes, composite
//! type names and the legacy aliases the shared parser rejects.

use super::{DataType, SqlError};

pub(in crate::sql::parser) fn parse_data_type(raw: &str) -> Result<DataType, SqlError> {
    let (base_type, is_sql_standard_array) = split_sql_standard_array_suffix(raw)?;
    let raw = base_type.unwrap_or(raw).trim();
    // SQL requires DOUBLE PRECISION and TEXT; preserve the shared parser's
    // legacy aliases for non-SQL callers, including array element names.
    if let Some(alias) = legacy_sql_type_alias(raw) {
        return Err(SqlError::unsupported(format!(
            "unsupported data type '{alias}'"
        )));
    }
    let parsed = DataType::parse_sql(raw);
    if parsed.is_err() {
        // `name[]` of a composite type: the array type row exists, so the
        // array is an unsupported feature rather than an undefined type.
        if let Some(element) = raw.strip_suffix("[]") {
            if super::super::type_scope::resolve_named_type(element.trim()).is_some() {
                return Err(SqlError::unsupported(
                    "arrays of composite types are not supported".into(),
                ));
            }
        }
    }
    if let (Err(_), Some(composite)) = (&parsed, super::super::type_scope::resolve_named_type(raw))
    {
        if is_sql_standard_array {
            return Err(SqlError::unsupported(
                "arrays of composite types are not supported".into(),
            ));
        }
        return Ok(DataType::Composite(composite));
    }
    let data_type = parsed.map_err(|message| {
        if message.starts_with("NUMERIC precision")
            || message.starts_with("NUMERIC scale")
            || message.starts_with("FLOAT precision")
        {
            SqlError::invalid_parameter_value(message)
        } else if message.starts_with("unsupported data type '") && !is_known_unselected_type(raw) {
            let name = raw.trim();
            SqlError::undefined_object(format!("type \"{name}\" does not exist"))
        } else {
            SqlError::new(message)
        }
    })?;
    Ok(if is_sql_standard_array {
        if matches!(data_type, DataType::Array(_) | DataType::JsonPath) {
            return Err(SqlError::new(if matches!(data_type, DataType::JsonPath) {
                "JSONPATH arrays are not supported".into()
            } else {
                "array-of-array types are not supported".into()
            }));
        }
        DataType::Array(Box::new(data_type))
    } else {
        data_type
    })
}

fn split_sql_standard_array_suffix(raw: &str) -> Result<(Option<&str>, bool), SqlError> {
    let Some(suffix_start) = sql_standard_array_suffix_start(raw) else {
        return Ok((None, false));
    };
    let base_type = raw[..suffix_start].trim_end();
    let suffix = raw[suffix_start + " array".len()..].trim();
    if base_type.is_empty() {
        return Err(SqlError::new(
            "array type suffix requires an element type".into(),
        ));
    }
    if suffix.is_empty() {
        return Ok((Some(base_type), true));
    }
    let Some(dimensions) = suffix
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Err(SqlError::new(format!(
            "invalid SQL array type dimensions '{suffix}'"
        )));
    };
    if dimensions.trim().parse::<u32>().is_err() {
        return Err(SqlError::new(format!(
            "invalid SQL array type dimensions '{suffix}'"
        )));
    }
    Ok((Some(base_type), true))
}

fn sql_standard_array_suffix_start(raw: &str) -> Option<usize> {
    let bytes = raw.as_bytes();
    let mut in_quoted_identifier = false;
    let mut depth = 0_usize;
    let mut suffix_start = None;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                if in_quoted_identifier && bytes.get(index + 1) == Some(&b'"') {
                    index += 2;
                    continue;
                }
                in_quoted_identifier = !in_quoted_identifier;
                index += 1;
            }
            b'(' if !in_quoted_identifier => {
                depth += 1;
                index += 1;
            }
            b')' if !in_quoted_identifier => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            _ if !in_quoted_identifier && depth == 0 && index + 5 <= bytes.len() => {
                let has_keyword_boundary = index > 0
                    && bytes[index - 1].is_ascii_whitespace()
                    && bytes[index..index + 5].eq_ignore_ascii_case(b"array")
                    && bytes
                        .get(index + 5)
                        .is_none_or(|byte| byte.is_ascii_whitespace() || *byte == b'[');
                if has_keyword_boundary {
                    suffix_start = Some(index - 1);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }

    suffix_start
}

fn is_known_unselected_type(raw: &str) -> bool {
    let mut type_name = raw.trim();
    type_name = type_name
        .strip_prefix("pg_catalog.")
        .unwrap_or(type_name)
        .trim_matches('"');
    if let Some((base, _)) = type_name.split_once('(') {
        type_name = base.trim();
    }
    if let Some(base) = type_name.strip_suffix("[]") {
        type_name = base.trim();
    }
    let type_name = type_name.to_ascii_lowercase();
    matches!(
        type_name.as_str(),
        "name"
            | "char"
            | "unknown"
            | "money"
            | "xml"
            | "point"
            | "line"
            | "lseg"
            | "box"
            | "path"
            | "polygon"
            | "circle"
            | "int4range"
            | "int8range"
            | "numrange"
            | "tsrange"
            | "tstzrange"
            | "daterange"
    )
}

fn legacy_sql_type_alias(raw: &str) -> Option<&'static str> {
    let raw = raw.trim();
    let element_type = raw.strip_suffix("[]").unwrap_or(raw).trim();
    if element_type.eq_ignore_ascii_case("double") {
        Some("double")
    } else if element_type.eq_ignore_ascii_case("string") {
        Some("string")
    } else {
        None
    }
}
