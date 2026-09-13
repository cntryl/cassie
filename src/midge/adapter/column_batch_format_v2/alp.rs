use crate::app::CassieError;

use super::binary::{invalid, write_i64, write_u32, Reader};
use super::bitpack;
use super::chunk::{checked_count, BLOCK_LEN};

const MAX_SCALE: u8 = 18;

pub(super) fn encode(values: &[&serde_json::Value]) -> Result<Vec<u8>, CassieError> {
    let mut selected = None;
    for scale in 0..=MAX_SCALE {
        let Some(scaled) = values
            .iter()
            .map(|value| scale_value(value, scale))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let payload = encode_scaled(&scaled, scale)?;
        if selected
            .as_ref()
            .is_none_or(|(_, current): &(u8, Vec<u8>)| payload.len() < current.len())
        {
            selected = Some((scale, payload));
        }
    }
    selected
        .map(|(_, payload)| payload)
        .ok_or_else(|| invalid("ALP requires finite exactly representable values"))
}

pub(super) fn decode(payload: &[u8], count: usize) -> Result<Vec<serde_json::Value>, CassieError> {
    decode_f64(payload, count)?
        .into_iter()
        .map(|value| {
            serde_json::Number::from_f64(value)
                .map(serde_json::Value::Number)
                .ok_or_else(|| invalid("invalid ALP decoded float"))
        })
        .collect()
}

pub(super) fn decode_f64(payload: &[u8], count: usize) -> Result<Vec<f64>, CassieError> {
    let (scale, scaled) = decode_scaled(payload, count)?;
    Ok(scaled
        .into_iter()
        .map(|value| scaled_to_f64(value, scale))
        .collect())
}

pub(super) fn decode_scaled(payload: &[u8], count: usize) -> Result<(u8, Vec<i64>), CassieError> {
    let mut reader = Reader::new(payload);
    let scale = reader.read_u8()?;
    if scale > MAX_SCALE || reader.read_u8()? != 0 {
        return Err(invalid("invalid ALP header"));
    }
    let block_count = checked_count(reader.read_u32()?)?;
    let expected_blocks = count.div_ceil(BLOCK_LEN);
    if block_count != expected_blocks {
        return Err(invalid("invalid ALP block count"));
    }
    let mut values = Vec::with_capacity(count);
    for block_index in 0..block_count {
        let block_len = usize::from(reader.read_u8()?);
        let width = reader.read_u8()?;
        let expected_len = if block_index + 1 == block_count {
            count.saturating_sub(block_index * BLOCK_LEN)
        } else {
            BLOCK_LEN
        };
        if block_len == 0 || block_len != expected_len || width > 64 {
            return Err(invalid("invalid ALP block"));
        }
        let base = reader.read_i64()?;
        let packed_len = reader.read_len(payload.len())?;
        bitpack::unpack_each(reader.read_exact(packed_len)?, block_len, width, |delta| {
            let scaled = i128::from(base)
                .checked_add(i128::from(delta))
                .ok_or_else(|| invalid("ALP scaled value overflow"))?;
            let scaled =
                i64::try_from(scaled).map_err(|_| invalid("ALP scaled value exceeds i64"))?;
            values.push(scaled);
            Ok(())
        })?;
    }
    reader.finish()?;
    Ok((scale, values))
}

fn scale_value(value: &serde_json::Value, scale: u8) -> Option<i64> {
    let value = value.as_f64()?;
    if !value.is_finite() || value == 0.0 && value.is_sign_negative() {
        return None;
    }
    let factor = scale_factor(scale);
    let scaled = value * factor;
    if !scaled.is_finite() {
        return None;
    }
    let scaled = format!("{scaled:.0}").parse::<i64>().ok()?;
    let reconstructed = scaled.to_string().parse::<f64>().ok()? / factor;
    (reconstructed.to_bits() == value.to_bits()).then_some(scaled)
}

fn encode_scaled(values: &[i64], scale: u8) -> Result<Vec<u8>, CassieError> {
    let mut out = Vec::new();
    out.push(scale);
    out.push(0);
    write_u32(
        &mut out,
        u32::try_from(values.len().div_ceil(BLOCK_LEN))
            .map_err(|_| invalid("too many ALP blocks"))?,
    );
    for block in values.chunks(BLOCK_LEN) {
        let base = *block
            .iter()
            .min()
            .ok_or_else(|| invalid("empty ALP block"))?;
        let deltas = block
            .iter()
            .map(|value| {
                u64::try_from(i128::from(*value) - i128::from(base))
                    .map_err(|_| invalid("ALP delta overflow"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let maximum = deltas.iter().copied().max().unwrap_or(0);
        let width = bitpack::required_bits(maximum);
        let packed = bitpack::pack(&deltas, width)?;
        out.push(u8::try_from(block.len()).map_err(|_| invalid("ALP block is too large"))?);
        out.push(width);
        write_i64(&mut out, base);
        write_u32(
            &mut out,
            u32::try_from(packed.len()).map_err(|_| invalid("ALP block is too large"))?,
        );
        out.extend_from_slice(&packed);
    }
    Ok(out)
}

fn scale_factor(scale: u8) -> f64 {
    10_f64.powi(i32::from(scale))
}

pub(crate) fn scale_value_at(value: &serde_json::Value, scale: u8) -> Option<i64> {
    if scale > MAX_SCALE {
        return None;
    }
    scale_value(value, scale)
}

pub(crate) fn scaled_to_f64(value: i64, scale: u8) -> f64 {
    i64_to_f64(value) / scale_factor(scale)
}

fn i64_to_f64(value: i64) -> f64 {
    const TWO_TO_32: f64 = 4_294_967_296.0;

    let magnitude = value.unsigned_abs();
    let high = u32::try_from(magnitude >> 32).expect("upper i64 bits should fit u32");
    let low =
        u32::try_from(magnitude & u64::from(u32::MAX)).expect("lower i64 bits should fit u32");
    let converted = f64::from(high).mul_add(TWO_TO_32, f64::from(low));
    if value.is_negative() {
        -converted
    } else {
        converted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_match_rust_integer_conversion_at_rounding_boundaries() {
        // Arrange
        let values = [
            i64::MIN,
            i64::MIN + 1,
            -(1_i64 << 53) - 1,
            -(1_i64 << 53),
            -(1_i64 << 53) + 1,
            -1,
            0,
            1,
            (1_i64 << 53) - 1,
            1_i64 << 53,
            (1_i64 << 53) + 1,
            i64::MAX - 1,
            i64::MAX,
        ];

        // Act
        let converted = values.map(i64_to_f64);
        let expected = values.map(|value| {
            value
                .to_string()
                .parse::<f64>()
                .expect("every i64 should parse as f64")
        });

        // Assert
        assert_eq!(converted.map(f64::to_bits), expected.map(f64::to_bits));
    }
}
