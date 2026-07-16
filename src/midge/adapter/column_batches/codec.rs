use std::collections::BTreeMap;

use crate::app::CassieError;
use crate::catalog::{ColumnBatchColumn, ColumnBatchPayload, ColumnBatchRow, ColumnBatchValueRun};

const MAGIC: &[u8; 4] = b"CCB1";
const CODEC_PLAIN: u8 = 0;
const CODEC_DICTIONARY_RLE: u8 = 1;

pub(super) fn encode_column_batch(payload: &ColumnBatchPayload) -> Result<Vec<u8>, CassieError> {
    let codec = match payload.codec_name.as_str() {
        "uncompressed" => CODEC_PLAIN,
        "dictionary_rle" => CODEC_DICTIONARY_RLE,
        name => {
            return Err(CassieError::Parse(format!(
                "unsupported column batch codec '{name}'"
            )))
        }
    };
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_varint(u64::from(payload.encoding_version), &mut out);
    write_varint(u64::from(payload.codec_version), &mut out);
    out.push(codec);
    match codec {
        CODEC_PLAIN => encode_rows(&payload.rows, &mut out),
        CODEC_DICTIONARY_RLE => encode_columns(payload, &mut out),
        _ => unreachable!("codec is validated above"),
    }
    Ok(out)
}

pub(super) fn decode_column_batch(bytes: &[u8]) -> Result<ColumnBatchPayload, CassieError> {
    let mut cursor = Cursor::new(bytes);
    cursor.expect(MAGIC)?;
    let encoding_version = cursor.read_u32()?;
    let codec_version = cursor.read_u32()?;
    let codec = cursor.read_byte()?;
    let payload = match codec {
        CODEC_PLAIN => ColumnBatchPayload {
            encoding_version,
            codec_name: "uncompressed".to_string(),
            codec_version,
            row_ids: Vec::new(),
            rows: decode_rows(&mut cursor)?,
            columns: Vec::new(),
        },
        CODEC_DICTIONARY_RLE => {
            let row_ids = cursor.read_strings()?;
            let column_count = cursor.read_usize()?;
            let mut columns = Vec::with_capacity(column_count);
            for _ in 0..column_count {
                let field = cursor.read_string()?;
                let run_count = cursor.read_usize()?;
                let mut runs = Vec::with_capacity(run_count);
                for _ in 0..run_count {
                    runs.push(ColumnBatchValueRun {
                        value: cursor.read_value()?,
                        len: cursor.read_usize()?,
                    });
                }
                columns.push(ColumnBatchColumn { field, runs });
            }
            ColumnBatchPayload {
                encoding_version,
                codec_name: "dictionary_rle".to_string(),
                codec_version,
                row_ids,
                rows: Vec::new(),
                columns,
            }
        }
        _ => {
            return Err(CassieError::Parse(
                "invalid column batch codec tag".to_string(),
            ))
        }
    };
    cursor.finish()?;
    Ok(payload)
}

fn encode_rows(rows: &[ColumnBatchRow], out: &mut Vec<u8>) {
    write_len(rows.len(), out);
    for row in rows {
        write_bytes(row.row_id.as_bytes(), out);
        write_len(row.values.len(), out);
        for (field, value) in &row.values {
            write_bytes(field.as_bytes(), out);
            encode_value(value, out);
        }
    }
}

fn decode_rows(cursor: &mut Cursor<'_>) -> Result<Vec<ColumnBatchRow>, CassieError> {
    let count = cursor.read_usize()?;
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        let row_id = cursor.read_string()?;
        let field_count = cursor.read_usize()?;
        let mut values = BTreeMap::new();
        for _ in 0..field_count {
            values.insert(cursor.read_string()?, cursor.read_value()?);
        }
        rows.push(ColumnBatchRow { row_id, values });
    }
    Ok(rows)
}

fn encode_columns(payload: &ColumnBatchPayload, out: &mut Vec<u8>) {
    write_len(payload.row_ids.len(), out);
    for row_id in &payload.row_ids {
        write_bytes(row_id.as_bytes(), out);
    }
    write_len(payload.columns.len(), out);
    for column in &payload.columns {
        write_bytes(column.field.as_bytes(), out);
        write_len(column.runs.len(), out);
        for run in &column.runs {
            encode_value(&run.value, out);
            write_len(run.len, out);
        }
    }
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
            for (name, value) in values {
                write_bytes(name.as_bytes(), out);
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
        if self.read_exact(expected.len())? == expected {
            Ok(())
        } else {
            Err(CassieError::Parse(
                "invalid column batch binary record".to_string(),
            ))
        }
    }

    fn read_byte(&mut self) -> Result<u8, CassieError> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, CassieError> {
        u32::try_from(self.read_varint()?)
            .map_err(|_| CassieError::Parse("column batch integer overflow".to_string()))
    }

    fn read_usize(&mut self) -> Result<usize, CassieError> {
        usize::try_from(self.read_varint()?)
            .map_err(|_| CassieError::Parse("column batch integer overflow".to_string()))
    }

    fn read_varint(&mut self) -> Result<u64, CassieError> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.read_byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(CassieError::Parse(
            "column batch varint overflow".to_string(),
        ))
    }

    fn read_string(&mut self) -> Result<String, CassieError> {
        String::from_utf8(self.read_bytes()?.to_vec())
            .map_err(|error| CassieError::Parse(format!("invalid column batch string: {error}")))
    }

    fn read_strings(&mut self) -> Result<Vec<String>, CassieError> {
        let count = self.read_usize()?;
        (0..count).map(|_| self.read_string()).collect()
    }

    fn read_bytes(&mut self) -> Result<&'a [u8], CassieError> {
        let len = self.read_usize()?;
        self.read_exact(len)
    }

    fn read_value(&mut self) -> Result<serde_json::Value, CassieError> {
        match self.read_byte()? {
            0 => Ok(serde_json::Value::Null),
            1 => Ok(serde_json::Value::Bool(false)),
            2 => Ok(serde_json::Value::Bool(true)),
            3 => Ok(serde_json::Value::from(i64::from_be_bytes(
                self.read_array()?,
            ))),
            4 => Ok(serde_json::Value::from(u64::from_be_bytes(
                self.read_array()?,
            ))),
            5 => {
                serde_json::Number::from_f64(f64::from_bits(u64::from_be_bytes(self.read_array()?)))
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| CassieError::Parse("invalid column batch float".to_string()))
            }
            6 => Ok(serde_json::Value::String(self.read_string()?)),
            7 => {
                let count = self.read_usize()?;
                let values = (0..count)
                    .map(|_| self.read_value())
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(serde_json::Value::Array(values))
            }
            8 => {
                let count = self.read_usize()?;
                let mut values = serde_json::Map::new();
                for _ in 0..count {
                    values.insert(self.read_string()?, self.read_value()?);
                }
                Ok(serde_json::Value::Object(values))
            }
            _ => Err(CassieError::Parse(
                "invalid column batch value tag".to_string(),
            )),
        }
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], CassieError> {
        self.read_exact(N)?
            .try_into()
            .map_err(|_| CassieError::Parse("truncated column batch number".to_string()))
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], CassieError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| CassieError::Parse("column batch offset overflow".to_string()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| CassieError::Parse("truncated column batch record".to_string()))?;
        self.offset = end;
        Ok(value)
    }

    fn finish(self) -> Result<(), CassieError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(CassieError::Parse(
                "trailing column batch bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_roundtrip_plain_typed_columns() {
        // Arrange
        let payload = ColumnBatchPayload {
            encoding_version: 1,
            codec_name: "uncompressed".to_string(),
            codec_version: 1,
            row_ids: Vec::new(),
            rows: vec![ColumnBatchRow {
                row_id: "r1".to_string(),
                values: BTreeMap::from([
                    ("active".to_string(), serde_json::json!(true)),
                    ("count".to_string(), serde_json::json!(7)),
                    ("label".to_string(), serde_json::json!("alpha")),
                    ("missing".to_string(), serde_json::Value::Null),
                ]),
            }],
            columns: Vec::new(),
        };

        // Act
        let encoded = encode_column_batch(&payload).expect("encode");
        let decoded = decode_column_batch(&encoded).expect("decode");

        // Assert
        assert_eq!(&encoded[..4], b"CCB1");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn should_roundtrip_dictionary_rle_columns() {
        // Arrange
        let payload = ColumnBatchPayload {
            encoding_version: 1,
            codec_name: "dictionary_rle".to_string(),
            codec_version: 1,
            row_ids: vec!["r1".to_string(), "r2".to_string()],
            rows: Vec::new(),
            columns: vec![ColumnBatchColumn {
                field: "kind".to_string(),
                runs: vec![ColumnBatchValueRun {
                    value: serde_json::json!("same"),
                    len: 2,
                }],
            }],
        };

        // Act
        let encoded = encode_column_batch(&payload).expect("encode");
        let decoded = decode_column_batch(&encoded).expect("decode");

        // Assert
        assert_eq!(decoded, payload);
    }
}
