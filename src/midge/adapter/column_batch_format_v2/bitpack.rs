use crate::app::CassieError;

use super::binary::invalid;

pub(super) fn required_bits(maximum: u64) -> u8 {
    u8::try_from(u64::BITS - maximum.leading_zeros()).expect("u64 bit width fits in u8")
}

pub(super) fn packed_len(count: usize, width: u8) -> Result<usize, CassieError> {
    if width > 64 {
        return Err(invalid("invalid column-batch bit width"));
    }
    count
        .checked_mul(usize::from(width))
        .and_then(|bits| bits.checked_add(7))
        .map(|bits| bits / 8)
        .ok_or_else(|| invalid("column-batch bit-packed length overflow"))
}

pub(super) fn pack(values: &[u64], width: u8) -> Result<Vec<u8>, CassieError> {
    let mut out = vec![0_u8; packed_len(values.len(), width)?];
    if width == 0 {
        if values.iter().any(|value| *value != 0) {
            return Err(invalid("non-zero value in zero-width block"));
        }
        return Ok(out);
    }
    let limit = if width == 64 {
        u64::MAX
    } else {
        (1_u64 << width) - 1
    };
    let mut bit_offset = 0usize;
    for value in values {
        if *value > limit {
            return Err(invalid("value exceeds column-batch bit width"));
        }
        for bit in 0..width {
            if value & (1_u64 << bit) != 0 {
                let position = bit_offset
                    .checked_add(usize::from(bit))
                    .ok_or_else(|| invalid("column-batch bit offset overflow"))?;
                out[position / 8] |= 1_u8 << (position % 8);
            }
        }
        bit_offset = bit_offset
            .checked_add(usize::from(width))
            .ok_or_else(|| invalid("column-batch bit offset overflow"))?;
    }
    Ok(out)
}

pub(super) fn unpack(bytes: &[u8], count: usize, width: u8) -> Result<Vec<u64>, CassieError> {
    if bytes.len() != packed_len(count, width)? {
        return Err(invalid("invalid column-batch bit-packed length"));
    }
    let mut values = Vec::with_capacity(count);
    let mut bit_offset = 0usize;
    for _ in 0..count {
        let mut value = 0_u64;
        for bit in 0..width {
            let position = bit_offset
                .checked_add(usize::from(bit))
                .ok_or_else(|| invalid("column-batch bit offset overflow"))?;
            if bytes[position / 8] & (1_u8 << (position % 8)) != 0 {
                value |= 1_u64 << bit;
            }
        }
        values.push(value);
        bit_offset = bit_offset
            .checked_add(usize::from(width))
            .ok_or_else(|| invalid("column-batch bit offset overflow"))?;
    }
    Ok(values)
}
