//! Explicit TEXT[] literal constructors over the existing logical array carrier.
use super::{Expr, SqlError};
use crate::types::DataType;

pub(super) fn parse(raw: &str, target: &DataType) -> Result<Option<Expr>, SqlError> {
    if *target != DataType::Array(Box::new(DataType::Text)) {
        return Ok(None);
    }
    let raw = raw.trim();
    let Some(prefix) = raw.get(..5) else {
        return Ok(None);
    };
    if !prefix.eq_ignore_ascii_case("array") {
        return Ok(None);
    }
    let body = raw[5..].trim();
    let Some(body) = body
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Ok(None);
    };
    let mut values = Vec::new();
    if !body.trim().is_empty() {
        for item in super::split_csv(body) {
            let item = item.trim();
            if item.eq_ignore_ascii_case("null") {
                values.push("NULL".to_owned());
            } else if let Some(value) = super::parse_text_array_element(item)? {
                values.push(format!(
                    "\"{}\"",
                    value.replace('\\', "\\\\").replace('"', "\\\"")
                ));
            } else {
                return Err(SqlError::unsupported(
                    "TEXT[] constructors currently require one-dimensional string or NULL literals"
                        .into(),
                ));
            }
        }
    }
    Ok(Some(Expr::StringLiteral(format!(
        "{{{}}}",
        values.join(",")
    ))))
}
