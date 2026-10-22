use crate::app::CassieError;
use crate::executor::ColumnMeta;
use crate::types::Value;

use super::request::{CopyRequest, CopyWireFormat, ForceQuote};

const BINARY_SIGNATURE: &[u8; 11] = b"PGCOPY\n\xff\r\n\0";

pub(super) fn response_format(format: CopyWireFormat) -> i16 {
    i16::from(format == CopyWireFormat::Binary)
}

pub(super) fn binary_header() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(BINARY_SIGNATURE.len() + 8);
    bytes.extend_from_slice(BINARY_SIGNATURE);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes
}

pub(super) fn binary_trailer() -> Vec<u8> {
    (-1_i16).to_be_bytes().to_vec()
}

pub(super) fn encode_header(request: &CopyRequest, columns: &[ColumnMeta]) -> Option<Vec<u8>> {
    if request.format != CopyWireFormat::Csv || !request.header {
        return None;
    }
    let mut fields = Vec::with_capacity(columns.len());
    for column in columns {
        fields.push(encode_csv_field(
            &column.name,
            request.csv_delimiter(),
            request.null_marker(),
            false,
        ));
    }
    let mut bytes = fields
        .join(&request.csv_delimiter().to_string())
        .into_bytes();
    bytes.push(b'\n');
    Some(bytes)
}

pub(super) fn encode_row(
    request: &CopyRequest,
    columns: &[ColumnMeta],
    row: &[Value],
) -> Result<Vec<u8>, CassieError> {
    if columns.len() != row.len() {
        return Err(CassieError::Execution(format!(
            "COPY result row has {} values but its description declares {} columns",
            row.len(),
            columns.len()
        )));
    }
    match request.format {
        CopyWireFormat::Text => Ok(encode_text_row(request, columns, row)),
        CopyWireFormat::Csv => Ok(encode_csv_row(request, columns, row)),
        CopyWireFormat::Binary => encode_binary_row(columns, row),
    }
}

fn encode_text_row(request: &CopyRequest, columns: &[ColumnMeta], row: &[Value]) -> Vec<u8> {
    let delimiter = request.text_delimiter();
    let mut bytes = Vec::new();
    for (index, value) in row.iter().enumerate() {
        if index != 0 {
            push_char(&mut bytes, delimiter);
        }
        match value {
            Value::Null => bytes.extend_from_slice(request.null_marker().as_bytes()),
            value => {
                let text = super::super::codecs::value_to_text(value, columns[index].type_oid);
                encode_text_field(&mut bytes, &text);
            }
        }
    }
    bytes.push(b'\n');
    bytes
}

fn encode_csv_row(request: &CopyRequest, columns: &[ColumnMeta], row: &[Value]) -> Vec<u8> {
    let delimiter = request.csv_delimiter();
    let mut fields = Vec::with_capacity(row.len());
    for (index, value) in row.iter().enumerate() {
        match value {
            Value::Null => fields.push(request.null_marker().to_string()),
            value => {
                let text = super::super::codecs::value_to_text(value, columns[index].type_oid);
                let force = should_force_quote(&request.force_quote, &columns[index].name);
                fields.push(encode_csv_field(
                    &text,
                    delimiter,
                    request.null_marker(),
                    force,
                ));
            }
        }
    }
    let mut bytes = fields.join(&delimiter.to_string()).into_bytes();
    bytes.push(b'\n');
    bytes
}

fn encode_binary_row(columns: &[ColumnMeta], row: &[Value]) -> Result<Vec<u8>, CassieError> {
    let count = i16::try_from(row.len())
        .map_err(|_| CassieError::ResourceLimit("COPY column count exceeds int16".into()))?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_be_bytes());
    for (value, column) in row.iter().zip(columns) {
        if matches!(value, Value::Null) {
            bytes.extend_from_slice(&(-1_i32).to_be_bytes());
            continue;
        }
        let encoded = super::super::codecs::value_to_binary(value, column.type_oid)
            .map_err(|error| CassieError::Unsupported(error.to_string()))?;
        let length = i32::try_from(encoded.len())
            .map_err(|_| CassieError::ResourceLimit("COPY field exceeds int32".into()))?;
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&encoded);
    }
    Ok(bytes)
}

fn encode_csv_field(value: &str, delimiter: char, null_marker: &str, force: bool) -> String {
    let quote = force
        || value == null_marker
        || value.contains(delimiter)
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r');
    if quote {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn should_force_quote(force_quote: &ForceQuote, name: &str) -> bool {
    match force_quote {
        ForceQuote::None => false,
        ForceQuote::All => true,
        ForceQuote::Columns(columns) => columns.iter().any(|column| column == name),
    }
}

fn encode_text_field(output: &mut Vec<u8>, value: &str) {
    for character in value.chars() {
        match character {
            '\\' => output.extend_from_slice(b"\\\\"),
            '\n' => output.extend_from_slice(b"\\n"),
            '\r' => output.extend_from_slice(b"\\r"),
            '\t' => output.extend_from_slice(b"\\t"),
            '\u{0008}' => output.extend_from_slice(b"\\b"),
            '\u{000c}' => output.extend_from_slice(b"\\f"),
            '\u{000b}' => output.extend_from_slice(b"\\v"),
            other => push_char(output, other),
        }
    }
}

fn push_char(output: &mut Vec<u8>, character: char) {
    let mut buffer = [0_u8; 4];
    output.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
}
