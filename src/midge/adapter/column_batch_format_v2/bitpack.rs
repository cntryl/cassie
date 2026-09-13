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
    let mut values = Vec::with_capacity(count);
    unpack_each(bytes, count, width, |value| {
        values.push(value);
        Ok(())
    })?;
    Ok(values)
}

pub(super) fn unpack_each(
    bytes: &[u8],
    count: usize,
    width: u8,
    mut consume: impl FnMut(u64) -> Result<(), CassieError>,
) -> Result<(), CassieError> {
    if bytes.len() != packed_len(count, width)? {
        return Err(invalid("invalid column-batch bit-packed length"));
    }
    if width == 0 {
        for _ in 0..count {
            consume(0)?;
        }
        return Ok(());
    }
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1_u64 << width) - 1
    };
    let mut input_position = 0usize;
    let mut buffered = 0_u128;
    let mut buffered_bits = 0_u8;
    for _ in 0..count {
        while buffered_bits < width {
            let byte = bytes
                .get(input_position)
                .copied()
                .ok_or_else(|| invalid("invalid column-batch bit-packed length"))?;
            buffered |= u128::from(byte) << buffered_bits;
            input_position = input_position
                .checked_add(1)
                .ok_or_else(|| invalid("column-batch bit offset overflow"))?;
            buffered_bits = buffered_bits
                .checked_add(8)
                .ok_or_else(|| invalid("column-batch bit offset overflow"))?;
        }
        consume(
            u64::try_from(buffered & u128::from(mask))
                .expect("masked column-batch value should fit u64"),
        )?;
        buffered >>= width;
        buffered_bits -= width;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{pack, unpack};

    #[test]
    fn should_roundtrip_every_bit_width_across_boundaries() {
        // Arrange
        for width in 0_u8..=64 {
            let mask = if width == 64 {
                u64::MAX
            } else if width == 0 {
                0
            } else {
                (1_u64 << width) - 1
            };
            for count in [0_usize, 1, 2, 7, 8, 9, 127, 128, 129] {
                let values = (0..count)
                    .map(|position| mixed(position) & mask)
                    .collect::<Vec<_>>();

                // Act
                let encoded = pack(&values, width).expect("pack deterministic values");
                let decoded = unpack(&encoded, count, width).expect("unpack deterministic values");

                // Assert
                assert_eq!(decoded, values, "width={width} count={count}");
            }
        }
    }

    #[test]
    fn should_reject_mismatched_packed_lengths() {
        // Arrange
        let values = [1_u64, 2, 3, 4];
        let mut encoded = pack(&values, 3).expect("pack values");
        encoded.push(0);

        // Act
        let trailing = unpack(&encoded, values.len(), 3);
        let truncated = unpack(&encoded[..1], values.len(), 3);

        // Assert
        assert!(trailing.is_err());
        assert!(truncated.is_err());
    }

    fn mixed(position: usize) -> u64 {
        let mut value = u64::try_from(position).expect("test position should fit u64");
        value ^= value >> 30;
        value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value ^= value >> 27;
        value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}
