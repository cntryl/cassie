//! Fixed-stack MAC and `pg_lsn` output for admitted binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{typed::TypedValue, Value};

fn mac(bytes: &[u8], output: &mut [u8; 23]) -> usize {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut index = 0;
    for (position, byte) in bytes.iter().copied().enumerate() {
        if position != 0 {
            output[index] = b':';
            index += 1;
        }
        output[index] = HEX[usize::from(byte >> 4)];
        output[index + 1] = HEX[usize::from(byte & 0x0f)];
        index += 2;
    }
    index
}

fn hex_word(mut value: u32, output: &mut [u8; 23], cursor: &mut usize) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut digits = [0_u8; 8];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = HEX[usize::try_from(value & 0x0f).expect("hex digit")];
        value >>= 4;
        if value == 0 {
            break;
        }
    }
    let count = digits.len() - start;
    output[*cursor..*cursor + count].copy_from_slice(&digits[start..]);
    *cursor += count;
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if !matches!(oid, 829 | 774 | 3220) {
        return Ok(None);
    }
    controls.check_interruption()?;
    let value = super::super::super::codecs::binary_to_value(bytes, oid).map_err(|error| {
        CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column + 1
        ))
    })?;
    controls.check_interruption()?;
    let mut output = [0_u8; 23];
    let length = match value {
        Value::Typed(TypedValue::Macaddr(bytes)) => mac(&bytes, &mut output),
        Value::Typed(TypedValue::Macaddr8(bytes)) => mac(&bytes, &mut output),
        Value::Typed(TypedValue::PgLsn(value)) => {
            let mut cursor = 0;
            let high = u32::try_from(value >> 32).expect("upper word fits");
            let low = u32::try_from(value & u64::from(u32::MAX)).expect("lower word fits");
            hex_word(high, &mut output, &mut cursor);
            output[cursor] = b'/';
            cursor += 1;
            hex_word(low, &mut output, &mut cursor);
            cursor
        }
        _ => {
            return Err(CassieError::Execution(
                "unexpected fixed binary network COPY value".into(),
            ))
        }
    };
    let memory = controls.reserve_query_memory(length)?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY network capacity".into(),
        ));
    }
    let formatted = std::str::from_utf8(&output[..length])
        .map_err(|_| CassieError::Execution("invalid binary COPY network output".into()))?;
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
