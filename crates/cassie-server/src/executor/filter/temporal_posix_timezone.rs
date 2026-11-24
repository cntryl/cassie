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
    let standard = offset(bytes, &mut index, 23, true)?;
    let end = index.checked_add(length)?;
    if bytes.get(index..end) != Some(&bytes[..length]) {
        return None;
    }
    index = end;
    if bytes.get(index) == Some(&b',') {
        if standard + 3_600 > 86_340 {
            return None;
        }
    } else {
        offset(bytes, &mut index, 23, true)?;
    }
    if take(bytes, &mut index)? != b',' {
        return None;
    }
    let first_month = month_rule(bytes, &mut index)?;
    if take(bytes, &mut index)? != b',' {
        return None;
    }
    let second_month = month_rule(bytes, &mut index)?;
    if index != bytes.len() || !(2..=10).contains(&first_month.abs_diff(second_month)) {
        return None;
    }
    Some(standard)
}

fn offset(bytes: &[u8], index: &mut usize, max_hours: i32, zero_seconds: bool) -> Option<i32> {
    let sign = match bytes.get(*index) {
        Some(b'-') => {
            *index += 1;
            -1
        }
        Some(b'+') => {
            *index += 1;
            1
        }
        _ => 1,
    };
    let hours = component(bytes, index)?;
    let mut minutes = 0;
    let mut seconds = 0;
    if bytes.get(*index) == Some(&b':') {
        *index += 1;
        let start = *index;
        minutes = component(bytes, index)?;
        if *index - start != 2 {
            return None;
        }
        if bytes.get(*index) == Some(&b':') {
            *index += 1;
            let start = *index;
            seconds = component(bytes, index)?;
            if *index - start != 2 {
                return None;
            }
        }
    }
    if hours > max_hours || minutes > 59 || seconds > 59 || (zero_seconds && seconds != 0) {
        return None;
    }
    Some(-sign * (hours * 3600 + minutes * 60 + seconds))
}

fn month_rule(bytes: &[u8], index: &mut usize) -> Option<u8> {
    if take(bytes, index)? != b'M' {
        return None;
    }
    let month = u8::try_from(component(bytes, index)?).ok()?;
    if !(1..=12).contains(&month) || take(bytes, index)? != b'.' {
        return None;
    }
    let week = take(bytes, index)?;
    if !(b'1'..=b'5').contains(&week) || take(bytes, index)? != b'.' {
        return None;
    }
    let day = take(bytes, index)?;
    if !(b'0'..=b'6').contains(&day) {
        return None;
    }
    Some(month)
}

fn take(bytes: &[u8], index: &mut usize) -> Option<u8> {
    let byte = *bytes.get(*index)?;
    *index += 1;
    Some(byte)
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
