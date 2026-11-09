//! A bounded POSIX duplicate-designation case with statement-owned provenance.
use super::QueryError;
use crate::runtime::QueryExecutionControls;
use crate::types::extended::temporal::{zone::Zone, TemporalSession};
use crate::types::typed::TypedValue;
use crate::types::Value;

pub(super) fn resolve(
    name: &Value,
    operand: &Value,
    session: &TemporalSession,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<Zone>, QueryError> {
    let (Value::String(name), Some(controls)) = (name, controls) else {
        return Ok(None);
    };
    if !matches!(
        operand,
        Value::Typed(TypedValue::Timestamp(_) | TypedValue::TimestampTz(_))
    ) {
        return Ok(None);
    }
    let Some(east) = fixed_duplicate_offset(session.zone.name(), name) else {
        return Ok(None);
    };
    if !controls
        .temporal_session_is_posix(session)
        .map_err(QueryError::Cassie)?
    {
        return Ok(None);
    }
    Ok(Some(Zone::Fixed {
        east,
        name: String::new(),
    }))
}

pub(super) fn fixed_duplicate_offset(rule: &str, name: &str) -> Option<i32> {
    if rule.len() > 255 || name.len() > 16 {
        return None;
    }
    let bytes = rule.as_bytes();
    let length = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_uppercase())
        .count();
    if !(3..=16).contains(&length) || !rule[..length].eq_ignore_ascii_case(name) {
        return None;
    }
    let mut index = length;
    let sign = match bytes.get(index) {
        Some(b'-') => {
            index += 1;
            -1
        }
        Some(b'+') => {
            index += 1;
            1
        }
        _ => 1,
    };
    let Some(hours) = component(bytes, &mut index) else {
        return None;
    };
    let mut minutes = 0;
    let mut seconds = 0;
    if bytes.get(index) == Some(&b':') {
        index += 1;
        let Some(value) = component(bytes, &mut index) else {
            return None;
        };
        minutes = value;
        if bytes.get(index) == Some(&b':') {
            index += 1;
            let Some(value) = component(bytes, &mut index) else {
                return None;
            };
            seconds = value;
        }
    }
    if hours > 23 || minutes > 59 || seconds != 0 {
        return None;
    }
    let Some(end) = index.checked_add(length) else {
        return None;
    };
    if bytes.get(index..end) != Some(&bytes[..length])
        || bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphabetic())
    {
        return None;
    }
    Some(-sign * (hours * 3600 + minutes * 60 + seconds))
}

fn component(bytes: &[u8], index: &mut usize) -> Option<i32> {
    let first = *bytes.get(*index)?;
    if !first.is_ascii_digit() {
        return None;
    }
    *index += 1;
    let mut value = i32::from(first - b'0');
    if let Some(second) = bytes.get(*index).filter(|byte| byte.is_ascii_digit()) {
        value = value * 10 + i32::from(*second - b'0');
        *index += 1;
    }
    Some(value)
}
