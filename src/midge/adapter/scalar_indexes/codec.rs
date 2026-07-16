use std::collections::BTreeMap;

use crate::app::CassieError;

const MAGIC: &[u8; 4] = b"SIC1";

pub(super) fn encode_covering_fields(fields: &BTreeMap<String, serde_json::Value>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_len(fields.len(), &mut out);
    for (field, value) in fields {
        write_bytes(field.as_bytes(), &mut out);
        encode_value(value, &mut out);
    }
    out
}

pub(super) fn decode_covering_fields(
    bytes: &[u8],
) -> Result<BTreeMap<String, serde_json::Value>, CassieError> {
    let mut cursor = Cursor::new(bytes);
    cursor.expect(MAGIC)?;
    let count = cursor.usize()?;
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        fields.insert(cursor.string()?, cursor.value()?);
    }
    cursor.finish()?;
    Ok(fields)
}

fn encode_value(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Null => out.push(0),
        serde_json::Value::Bool(false) => out.push(1),
        serde_json::Value::Bool(true) => out.push(2),
        serde_json::Value::Number(number) if number.is_i64() => {
            out.push(3);
            out.extend_from_slice(&number.as_i64().unwrap_or_default().to_be_bytes());
        }
        serde_json::Value::Number(number) if number.is_u64() => {
            out.push(4);
            out.extend_from_slice(&number.as_u64().unwrap_or_default().to_be_bytes());
        }
        serde_json::Value::Number(number) => {
            out.push(5);
            out.extend_from_slice(&number.as_f64().unwrap_or_default().to_bits().to_be_bytes());
        }
        serde_json::Value::String(value) => {
            out.push(6);
            write_bytes(value.as_bytes(), out);
        }
        serde_json::Value::Array(values) => {
            out.push(7);
            write_len(values.len(), out);
            for value in values {
                encode_value(value, out);
            }
        }
        serde_json::Value::Object(values) => {
            out.push(8);
            write_len(values.len(), out);
            for (field, value) in values {
                write_bytes(field.as_bytes(), out);
                encode_value(value, out);
            }
        }
    }
}

fn write_len(value: usize, out: &mut Vec<u8>) {
    write_varint(u64::try_from(value).unwrap_or(u64::MAX), out);
}

fn write_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    write_len(bytes.len(), out);
    out.extend_from_slice(bytes);
}

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(u8::try_from(value & 0x7f).expect("masked varint byte") | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("final varint byte"));
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect(&mut self, expected: &[u8]) -> Result<(), CassieError> {
        if self.take(expected.len())? == expected {
            Ok(())
        } else {
            Err(CassieError::Parse(
                "invalid scalar covering record".to_string(),
            ))
        }
    }

    fn byte(&mut self) -> Result<u8, CassieError> {
        Ok(self.take(1)?[0])
    }

    fn usize(&mut self) -> Result<usize, CassieError> {
        usize::try_from(self.varint()?)
            .map_err(|_| CassieError::Parse("scalar covering integer overflow".to_string()))
    }

    fn varint(&mut self) -> Result<u64, CassieError> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(CassieError::Parse(
            "scalar covering varint overflow".to_string(),
        ))
    }

    fn string(&mut self) -> Result<String, CassieError> {
        let len = self.usize()?;
        String::from_utf8(self.take(len)?.to_vec())
            .map_err(|error| CassieError::Parse(format!("invalid covering string: {error}")))
    }

    fn value(&mut self) -> Result<serde_json::Value, CassieError> {
        match self.byte()? {
            0 => Ok(serde_json::Value::Null),
            1 => Ok(serde_json::Value::Bool(false)),
            2 => Ok(serde_json::Value::Bool(true)),
            3 => Ok(serde_json::Value::from(i64::from_be_bytes(self.array()?))),
            4 => Ok(serde_json::Value::from(u64::from_be_bytes(self.array()?))),
            5 => serde_json::Number::from_f64(f64::from_bits(u64::from_be_bytes(self.array()?)))
                .map(serde_json::Value::Number)
                .ok_or_else(|| CassieError::Parse("invalid covering float".to_string())),
            6 => Ok(serde_json::Value::String(self.string()?)),
            7 => {
                let count = self.usize()?;
                Ok(serde_json::Value::Array(
                    (0..count)
                        .map(|_| self.value())
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
            8 => {
                let count = self.usize()?;
                let mut values = serde_json::Map::new();
                for _ in 0..count {
                    values.insert(self.string()?, self.value()?);
                }
                Ok(serde_json::Value::Object(values))
            }
            _ => Err(CassieError::Parse(
                "invalid scalar covering value tag".to_string(),
            )),
        }
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CassieError> {
        self.take(N)?
            .try_into()
            .map_err(|_| CassieError::Parse("truncated scalar covering number".to_string()))
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CassieError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| CassieError::Parse("scalar covering offset overflow".to_string()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| CassieError::Parse("truncated scalar covering record".to_string()))?;
        self.offset = end;
        Ok(value)
    }

    fn finish(self) -> Result<(), CassieError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(CassieError::Parse(
                "trailing scalar covering bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_roundtrip_compact_covering_values_without_row_identity() {
        // Arrange
        let fields = BTreeMap::from([
            ("active".to_string(), serde_json::json!(true)),
            ("count".to_string(), serde_json::json!(3)),
            ("title".to_string(), serde_json::json!("alpha")),
        ]);

        // Act
        let encoded = encode_covering_fields(&fields);
        let decoded = decode_covering_fields(&encoded).expect("decode fields");

        // Assert
        assert_eq!(&encoded[..4], b"SIC1");
        assert_eq!(decoded, fields);
    }
}
