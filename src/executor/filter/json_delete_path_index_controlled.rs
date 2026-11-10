//! Interruptible equivalent of the deletion path's INT4 index parser.
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;

#[cfg(test)]
#[path = "json_delete_path_index_controlled_tests.rs"]
mod tests;

pub(super) fn parse(
    element: &str,
    length: usize,
    position: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<usize>, QueryError> {
    parse_observing(element, length, position, controls, || {})
}

fn parse_observing(
    element: &str,
    length: usize,
    position: usize,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(),
) -> Result<Option<usize>, QueryError> {
    let invalid = || {
        super::super::super::super::type_input_error(
            "22P02",
            format!("path element at position {position} is not an integer: \"{element}\""),
        )
    };
    let bytes = element.as_bytes();
    let mut index = 0;
    controls.check_interruption()?;
    while matches!(
        bytes.get(index),
        Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
    ) {
        controls.check_interruption()?;
        index += 1;
        observe();
        controls.check_interruption()?;
    }
    let negative = match bytes.get(index) {
        Some(b'-') => {
            index += 1;
            observe();
            controls.check_interruption()?;
            true
        }
        Some(b'+') => {
            index += 1;
            observe();
            controls.check_interruption()?;
            false
        }
        _ => false,
    };
    let start = index;
    let maximum = if negative {
        2_147_483_648_i64
    } else {
        2_147_483_647_i64
    };
    let mut value = 0_i64;
    while let Some(byte) = bytes.get(index).copied() {
        controls.check_interruption()?;
        index += 1;
        observe();
        controls.check_interruption()?;
        if !byte.is_ascii_digit() {
            return Err(invalid());
        }
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i64::from(byte - b'0')))
            .filter(|value| *value <= maximum)
            .ok_or_else(invalid)?;
    }
    if index == start {
        return Err(invalid());
    }
    if negative {
        value = -value;
    }
    Ok(super::super::super::super::normalize_array_index(
        value, length,
    ))
}
