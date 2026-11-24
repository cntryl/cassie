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
    decode_with_selection(payload, count, None)
}

pub(super) fn decode_selected(
    payload: &[u8],
    count: usize,
    selection: &[bool],
) -> Result<Vec<serde_json::Value>, CassieError> {
    if selection.len() != count {
        return Err(invalid("FSST selection length mismatch"));
    }
    decode_with_selection(payload, count, Some(selection))
}

fn decode_with_selection(
    payload: &[u8],
    count: usize,
    selection: Option<&[bool]>,
) -> Result<Vec<serde_json::Value>, CassieError> {
    let mut reader = Reader::new(payload);
    let symbols = decode_symbol_table(&mut reader)?;
    let value_count =
        usize::try_from(reader.read_u32()?).map_err(|_| invalid("FSST value count overflow"))?;
    if value_count != count {
        return Err(invalid("FSST value count mismatch"));
    }
    let mut values = Vec::with_capacity(count);
    let mut decoded = Vec::new();
    for position in 0..count {
        let encoded = reader.read_bounded_bytes(MAX_SCALAR_BYTES)?;
        if selection.is_none_or(|selection| selection[position]) {
            decode_value(encoded, &symbols, Some(&mut decoded))?;
            let value = std::str::from_utf8(&decoded).map_err(|_| invalid("invalid FSST UTF-8"))?;
            values.push(serde_json::Value::String(value.to_owned()));
        } else {
            decode_value(encoded, &symbols, None)?;
            values.push(serde_json::Value::Null);
        }
    }
    reader.finish()?;
    Ok(values)
}

fn decode_symbol_table<'a>(reader: &mut Reader<'a>) -> Result<Vec<&'a [u8]>, CassieError> {
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
        symbols.push(symbol);
    }
    Ok(symbols)
}

fn decode_value(
    encoded: &[u8],
    symbols: &[&[u8]],
    mut decoded: Option<&mut Vec<u8>>,
) -> Result<(), CassieError> {
    if let Some(decoded) = decoded.as_deref_mut() {
        decoded.clear();
    }
    let mut stream = Reader::new(encoded);
    let mut utf8 = Utf8Validator::default();
    let mut decoded_len = 0usize;
    while stream.remaining() > 0 {
        let token = stream.read_u16()?;
        let (bytes, known_utf8) = if token == ESCAPE {
            (stream.read_bounded_bytes(MAX_SCALAR_BYTES)?, false)
        } else {
            (
                symbols
                    .get(usize::from(token))
                    .copied()
                    .ok_or_else(|| invalid("FSST symbol index out of range"))?,
                true,
            )
        };
        decoded_len = decoded_len
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("FSST value length overflow"))?;
        if decoded_len > MAX_SCALAR_BYTES {
            return Err(invalid("FSST value exceeds limit"));
        }
        if known_utf8 {
            utf8.push_known_valid()?;
        } else {
            utf8.push(bytes)?;
        }
        if let Some(decoded) = decoded.as_deref_mut() {
            decoded.extend_from_slice(bytes);
        }
    }
    stream.finish()?;
    utf8.finish()
}

#[derive(Default)]
struct Utf8Validator {
    remaining: u8,
    next_minimum: u8,
    next_maximum: u8,
}

impl Utf8Validator {
    fn push(&mut self, bytes: &[u8]) -> Result<(), CassieError> {
        for byte in bytes {
            if self.remaining == 0 {
                match *byte {
                    0x00..=0x7f => {}
                    0xc2..=0xdf => self.begin(1, 0x80, 0xbf),
                    0xe0 => self.begin(2, 0xa0, 0xbf),
                    0xe1..=0xec | 0xee..=0xef => self.begin(2, 0x80, 0xbf),
                    0xed => self.begin(2, 0x80, 0x9f),
                    0xf0 => self.begin(3, 0x90, 0xbf),
                    0xf1..=0xf3 => self.begin(3, 0x80, 0xbf),
                    0xf4 => self.begin(3, 0x80, 0x8f),
                    _ => return Err(invalid("invalid FSST UTF-8")),
                }
                continue;
            }
            if *byte < self.next_minimum || *byte > self.next_maximum {
                return Err(invalid("invalid FSST UTF-8"));
            }
            self.remaining -= 1;
            self.next_minimum = 0x80;
            self.next_maximum = 0xbf;
        }
        Ok(())
    }

    const fn begin(&mut self, remaining: u8, next_minimum: u8, next_maximum: u8) {
        self.remaining = remaining;
        self.next_minimum = next_minimum;
        self.next_maximum = next_maximum;
    }

    fn push_known_valid(&self) -> Result<(), CassieError> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(invalid("invalid FSST UTF-8"))
        }
    }

    fn finish(self) -> Result<(), CassieError> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(invalid("invalid FSST UTF-8"))
        }
    }
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

    use super::{decode, decode_selected, encode, ESCAPE};

    #[test]
    fn should_materialize_only_selected_fsst_values() {
        // Arrange
        let values = [
            serde_json::json!("tenant-a/event-created"),
            serde_json::json!("tenant-b/event-updated"),
            serde_json::json!("tenant-c/event-deleted"),
        ];
        let references = values.iter().collect::<Vec<_>>();
        let encoded = encode(&references).expect("encode FSST values");

        // Act
        let decoded = decode_selected(&encoded, values.len(), &[true, false, true])
            .expect("decode selected FSST values");

        // Assert
        assert_eq!(
            decoded,
            vec![
                values[0].clone(),
                serde_json::Value::Null,
                values[2].clone()
            ]
        );
    }

    #[test]
    fn should_reject_corrupt_unselected_fsst_value() {
        // Arrange
        let mut payload = Vec::new();
        payload.extend_from_slice(&1_u16.to_le_bytes());
        payload.extend_from_slice(&5_u32.to_le_bytes());
        payload.extend_from_slice(b"hello");
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&1_u16.to_le_bytes());

        // Act
        let decoded = decode_selected(&payload, 2, &[true, false]);

        // Assert
        assert!(decoded.is_err());
    }

    #[test]
    fn should_reject_invalid_utf8_in_unselected_fsst_value() {
        // Arrange
        let mut payload = Vec::new();
        payload.extend_from_slice(&0_u16.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        for byte in [b'a', 0xff] {
            payload.extend_from_slice(&7_u32.to_le_bytes());
            payload.extend_from_slice(&ESCAPE.to_le_bytes());
            payload.extend_from_slice(&1_u32.to_le_bytes());
            payload.push(byte);
        }

        // Act
        let decoded = decode_selected(&payload, 2, &[true, false]);

        // Assert
        assert!(decoded.is_err());
    }

    #[test]
    fn should_decode_utf8_split_across_fsst_literals() {
        // Arrange
        let mut encoded_value = Vec::new();
        for byte in [0xc2, 0xa2] {
            encoded_value.extend_from_slice(&ESCAPE.to_le_bytes());
            encoded_value.extend_from_slice(&1_u32.to_le_bytes());
            encoded_value.push(byte);
        }
        let mut payload = Vec::new();
        payload.extend_from_slice(&0_u16.to_le_bytes());
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(
            &u32::try_from(encoded_value.len())
                .expect("encoded value length")
                .to_le_bytes(),
        );
        payload.extend_from_slice(&encoded_value);

        // Act
        let decoded = decode_selected(&payload, 1, &[true]).expect("decode split UTF-8");

        // Assert
        assert_eq!(decoded, vec![serde_json::json!("¢")]);
    }

    #[test]
    fn should_reject_incomplete_utf8_before_fsst_symbol() {
        // Arrange
        let mut encoded_value = Vec::new();
        encoded_value.extend_from_slice(&ESCAPE.to_le_bytes());
        encoded_value.extend_from_slice(&1_u32.to_le_bytes());
        encoded_value.push(0xc2);
        encoded_value.extend_from_slice(&0_u16.to_le_bytes());
        let mut payload = Vec::new();
        payload.extend_from_slice(&1_u16.to_le_bytes());
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.push(b'a');
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(
            &u32::try_from(encoded_value.len())
                .expect("encoded value length")
                .to_le_bytes(),
        );
        payload.extend_from_slice(&encoded_value);

        // Act
        let decoded = decode_selected(&payload, 1, &[false]);

        // Assert
        assert!(decoded.is_err());
    }

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
