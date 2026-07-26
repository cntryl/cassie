use std::collections::BTreeMap;

use crate::app::CassieError;

use super::binary::{invalid, write_bytes, write_u16, write_u32, Reader};
use super::chunk::MAX_SCALAR_BYTES;

const MAX_SYMBOLS: usize = 256;
const ESCAPE: u16 = u16::MAX;

pub(super) fn encode(values: &[&serde_json::Value]) -> Result<Vec<u8>, CassieError> {
    let strings = values
        .iter()
        .map(|value| value.as_str().ok_or_else(|| invalid("FSST text expected")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut candidates = BTreeMap::<Vec<u8>, usize>::new();
    for value in &strings {
        let bytes = value.as_bytes();
        for start in 0..bytes.len() {
            for length in 3..=16 {
                let end = start.saturating_add(length);
                let Some(symbol) = bytes.get(start..end) else {
                    break;
                };
                if std::str::from_utf8(symbol).is_ok() {
                    *candidates.entry(symbol.to_vec()).or_default() += 1;
                }
            }
        }
    }
    let mut symbols = candidates
        .into_iter()
        .filter_map(|(symbol, count)| {
            let gain = count.saturating_mul(symbol.len().saturating_sub(2));
            (gain > 0).then_some((gain, symbol))
        })
        .collect::<Vec<_>>();
    symbols.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    symbols.truncate(MAX_SYMBOLS);
    let symbols = symbols
        .into_iter()
        .map(|(_, symbol)| symbol)
        .collect::<Vec<_>>();
    let lookup = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| {
            (
                symbol.as_slice(),
                u16::try_from(index).expect("FSST index fits"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut out = Vec::new();
    write_u16(
        &mut out,
        u16::try_from(symbols.len()).expect("FSST symbol count fits"),
    );
    for symbol in &symbols {
        write_bytes(&mut out, symbol)?;
    }
    write_u32(
        &mut out,
        u32::try_from(strings.len()).map_err(|_| invalid("too many FSST values"))?,
    );
    for value in strings {
        let encoded = encode_value(value.as_bytes(), &symbols, &lookup);
        write_bytes(&mut out, encoded.as_slice())?;
    }
    Ok(out)
}

pub(super) fn decode(payload: &[u8], count: usize) -> Result<Vec<serde_json::Value>, CassieError> {
    let mut reader = Reader::new(payload);
    let symbol_count = usize::from(reader.read_u16()?);
    if symbol_count > MAX_SYMBOLS {
        return Err(invalid("invalid FSST symbol count"));
    }
    let mut symbols = Vec::with_capacity(symbol_count);
    let mut table_bytes = 0usize;
    for _ in 0..symbol_count {
        let symbol = reader.read_bounded_bytes(MAX_SCALAR_BYTES)?;
        table_bytes = table_bytes.saturating_add(symbol.len());
        if table_bytes > 64 * 1024 || symbol.is_empty() || std::str::from_utf8(symbol).is_err() {
            return Err(invalid("invalid FSST symbol table"));
        }
        symbols.push(symbol.to_vec());
    }
    let value_count =
        usize::try_from(reader.read_u32()?).map_err(|_| invalid("FSST value count overflow"))?;
    if value_count != count {
        return Err(invalid("FSST value count mismatch"));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let encoded = reader.read_bounded_bytes(MAX_SCALAR_BYTES)?;
        let mut value = Vec::new();
        let mut stream = Reader::new(encoded);
        while stream.remaining() > 0 {
            let token = stream.read_u16()?;
            if token == ESCAPE {
                let literal = stream.read_bounded_bytes(MAX_SCALAR_BYTES)?;
                value.extend_from_slice(literal);
            } else {
                let symbol = symbols
                    .get(usize::from(token))
                    .ok_or_else(|| invalid("FSST symbol index out of range"))?;
                value.extend_from_slice(symbol);
            }
            if value.len() > MAX_SCALAR_BYTES {
                return Err(invalid("FSST value exceeds limit"));
            }
        }
        stream.finish()?;
        let value = String::from_utf8(value).map_err(|_| invalid("invalid FSST UTF-8"))?;
        values.push(serde_json::Value::String(value));
    }
    reader.finish()?;
    Ok(values)
}

fn encode_value(value: &[u8], symbols: &[Vec<u8>], lookup: &BTreeMap<&[u8], u16>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut position = 0;
    let mut literal_start = 0;
    while position < value.len() {
        let best = symbols
            .iter()
            .enumerate()
            .filter_map(|(index, symbol)| {
                value
                    .get(position..position.saturating_add(symbol.len()))
                    .filter(|candidate| *candidate == symbol.as_slice())
                    .map(|_| (symbol.len(), index))
            })
            .max_by_key(|(length, _)| *length);
        let Some((length, index)) = best else {
            position += 1;
            continue;
        };
        if literal_start < position {
            write_u16(&mut out, ESCAPE);
            write_bytes(&mut out, &value[literal_start..position]).expect("literal length fits");
        }
        write_u16(
            &mut out,
            *lookup
                .get(symbols[index].as_slice())
                .expect("symbol lookup"),
        );
        position += length;
        literal_start = position;
    }
    if literal_start < value.len() {
        write_u16(&mut out, ESCAPE);
        write_bytes(&mut out, &value[literal_start..]).expect("literal length fits");
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};

    use super::{decode, encode};

    #[test]
    fn should_roundtrip_deterministic_golden_payload() {
        // Arrange
        let values = [
            serde_json::json!("tenant-a/event-created"),
            serde_json::json!("tenant-b/event-created"),
            serde_json::json!("tenant-a/event-created"),
            serde_json::json!("tenant-b/event-created"),
            serde_json::json!("tenant-a/event-created"),
            serde_json::json!("tenant-b/event-created"),
            serde_json::json!("tenant-a/event-created"),
            serde_json::json!("tenant-b/event-created"),
        ];
        let references = values.iter().collect::<Vec<_>>();

        // Act
        let encoded = encode(&references).expect("encode FSST payload");

        // Assert
        let digest = Sha256::digest(&encoded);
        let mut digest_hex = String::with_capacity(64);
        for byte in digest {
            write!(&mut digest_hex, "{byte:02x}").expect("digest formatting");
        }
        println!(
            "FSST payload fixture: len={}, sha256={digest_hex}",
            encoded.len()
        );
        assert_eq!(encoded.len(), 3_551);
        assert_eq!(
            digest_hex,
            "8557d87a09e9cfc49a84b7fb96953ab07490fd8e687f318782a0cde564ee8ad3"
        );
        assert_eq!(
            decode(&encoded, values.len()).expect("decode FSST payload"),
            values
        );
    }
}
