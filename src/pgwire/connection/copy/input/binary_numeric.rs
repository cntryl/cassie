//! Checked base-10000 NUMERIC output for admitted binary COPY fields.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "binary_numeric_tests.rs"]
mod tests;

fn invalid(column: usize, short: bool) -> CassieError {
    let message = if short {
        "invalid binary numeric value"
    } else {
        "incorrect binary data format for numeric"
    };
    CassieError::Parse(format!(
        "COPY binary field {} is invalid: {message}",
        column + 1
    ))
}

fn new_text(
    length: usize,
    controls: &QueryExecutionControls,
) -> Result<(String, QueryMemoryReservation), CassieError> {
    let memory = controls.reserve_query_memory(length)?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY numeric capacity".into(),
        ));
    }
    Ok((text, memory))
}

fn group(bytes: &[u8], weight: i32, exponent: i32) -> u16 {
    let index = weight - exponent;
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let Some(offset) = index.checked_mul(2).and_then(|n| n.checked_add(8)) else {
        return 0;
    };
    bytes
        .get(offset..offset + 2)
        .map_or(0, |pair| u16::from_be_bytes([pair[0], pair[1]]))
}

fn append_group(text: &mut String, mut digit: u16, width: usize) {
    let mut bytes = [b'0'; 4];
    for byte in bytes.iter_mut().rev() {
        *byte += u8::try_from(digit % 10).expect("decimal digit");
        digit /= 10;
    }
    for byte in &bytes[4 - width..] {
        text.push(char::from(*byte));
    }
}

fn width(digit: u16) -> usize {
    if digit >= 1000 {
        4
    } else if digit >= 100 {
        3
    } else if digit >= 10 {
        2
    } else {
        1
    }
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    decode_with_observer(oid, bytes, column, controls, || {})
}

#[cfg(test)]
fn decode_observed(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    observer: impl FnMut(),
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    decode_with_observer(oid, bytes, column, controls, observer)
}

fn decode_with_observer(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    mut observer: impl FnMut(),
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if oid != 1700 {
        return Ok(None);
    }
    controls.check_interruption()?;
    let Some(header) = bytes.get(..8) else {
        return Err(invalid(column, true));
    };
    let count = usize::from(u16::from_be_bytes([header[0], header[1]]));
    let weight = i32::from(i16::from_be_bytes([header[2], header[3]]));
    let sign = u16::from_be_bytes([header[4], header[5]]);
    let scale = usize::from(u16::from_be_bytes([header[6], header[7]]));
    if bytes.len() != 8 + count * 2 {
        return Err(invalid(column, false));
    }
    let special = match sign {
        0xc000 => Some("NaN"),
        0xd000 => Some("Infinity"),
        0xf000 => Some("-Infinity"),
        0 | 0x4000 => None,
        _ => return Err(invalid(column, false)),
    };
    if let Some(special) = special {
        let memory;
        let mut text;
        (text, memory) = new_text(special.len(), controls)?;
        text.push_str(special);
        controls.check_interruption()?;
        return Ok(Some((text, memory)));
    }
    if scale > usize::try_from(crate::types::extended::numeric::MAX_DSCALE).expect("scale fits") {
        return Err(invalid(column, false));
    }
    let fraction_groups = scale.div_ceil(4);
    let (first_integer, nonzero) =
        scan_numeric_groups(bytes, count, weight, scale, controls, &mut observer)?;
    controls.check_interruption()?;
    let integer_length = first_integer.map_or(1, |(exponent, digit)| {
        width(digit) + usize::try_from(exponent).expect("nonnegative exponent") * 4
    });
    let negative = sign == 0x4000 && nonzero;
    let length = integer_length
        .checked_add(usize::from(negative))
        .and_then(|length| length.checked_add(if scale == 0 { 0 } else { scale + 1 }))
        .ok_or_else(|| CassieError::ResourceLimit("COPY numeric output length overflow".into()))?;
    let memory;
    let mut text;
    (text, memory) = new_text(length, controls)?;
    if negative {
        text.push('-');
    }
    if let Some((exponent, digit)) = first_integer {
        append_group(&mut text, digit, width(digit));
        let groups = usize::try_from(exponent).expect("nonnegative exponent");
        for remaining in (0..groups).rev() {
            if remaining.is_multiple_of(128) {
                observer();
                controls.check_interruption()?;
            }
            append_group(
                &mut text,
                group(
                    bytes,
                    weight,
                    i32::try_from(remaining).expect("weight fits"),
                ),
                4,
            );
        }
    } else {
        text.push('0');
    }
    if scale != 0 {
        text.push('.');
        let mut written = 0;
        for index in 0..fraction_groups {
            if index.is_multiple_of(128) {
                if index != 0 {
                    observer();
                }
                controls.check_interruption()?;
            }
            let exponent = -i32::try_from(index + 1).expect("fraction group count");
            let digit = group(bytes, weight, exponent);
            let mut group_text = [b'0'; 4];
            let mut value = digit;
            for byte in group_text.iter_mut().rev() {
                *byte += u8::try_from(value % 10).expect("decimal digit");
                value /= 10;
            }
            let take = (scale - written).min(4);
            for byte in &group_text[..take] {
                text.push(char::from(*byte));
            }
            written += take;
        }
    }
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}

fn scan_numeric_groups(
    bytes: &[u8],
    count: usize,
    weight: i32,
    scale: usize,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(),
) -> Result<(Option<(i32, u16)>, bool), CassieError> {
    let fraction_groups = scale.div_ceil(4);
    let mut first_integer = None;
    let mut nonzero = false;
    for index in 0..count {
        if index.is_multiple_of(128) {
            if index != 0 {
                observer();
            }
            controls.check_interruption()?;
        }
        let offset = 8 + index * 2;
        let digit = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        if digit >= 10_000 {
            return Err(invalid(column, false));
        }
        let exponent = weight - i32::try_from(index).expect("u16 group count");
        if exponent >= 0 && digit != 0 {
            if first_integer.is_none() {
                first_integer = Some((exponent, digit));
            }
            nonzero = true;
        } else if exponent < 0 {
            let position = usize::try_from(-exponent).expect("negative exponent");
            if position <= fraction_groups {
                let retained = if position == fraction_groups && !scale.is_multiple_of(4) {
                    scale % 4
                } else {
                    4
                };
                if digit / 10_u16.pow(u32::try_from(4 - retained).expect("decimal width")) != 0 {
                    nonzero = true;
                }
            }
        }
    }
    Ok((first_integer, nonzero))
}
