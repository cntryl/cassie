use crate::app::CassieError;

use super::binary::{invalid, write_bytes, write_u16, write_u32, write_u64, Reader};

const MAGIC: &[u8; 4] = b"CBR2";
const FORMAT_VERSION: u16 = 2;
const MAX_ROW_IDS: usize = 1_048_576;
const MAX_ROW_ID_BYTES: usize = 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn encode(row_ids: &[String]) -> Result<Vec<u8>, CassieError> {
    if row_ids.len() > MAX_ROW_IDS {
        return Err(invalid("column-batch row-ID count exceeds limit"));
    }
    let mut payload = Vec::new();
    for row_id in row_ids {
        if row_id.len() > MAX_ROW_ID_BYTES {
            return Err(invalid("column-batch row ID exceeds limit"));
        }
        write_bytes(&mut payload, row_id.as_bytes())?;
    }
    let decoded_len = row_ids
        .iter()
        .try_fold(0usize, |total, row_id| total.checked_add(row_id.len()));
    let decoded_len = decoded_len.ok_or_else(|| invalid("column-batch row-ID length overflow"))?;
    let capacity = 22usize
        .checked_add(payload.len())
        .ok_or_else(|| invalid("column-batch row-ID chunk length overflow"))?;
    if capacity > MAX_CHUNK_BYTES {
        return Err(invalid("column-batch row-ID chunk exceeds limit"));
    }
    let mut out = Vec::with_capacity(capacity);
    out.extend_from_slice(MAGIC);
    write_u16(&mut out, FORMAT_VERSION);
    write_u32(
        &mut out,
        u32::try_from(row_ids.len())
            .map_err(|_| invalid("column-batch row-ID count exceeds u32"))?,
    );
    write_u32(
        &mut out,
        u32::try_from(payload.len())
            .map_err(|_| invalid("column-batch row-ID payload exceeds u32"))?,
    );
    write_u64(
        &mut out,
        u64::try_from(decoded_len)
            .map_err(|_| invalid("column-batch row-ID decoded length exceeds u64"))?,
    );
    out.extend_from_slice(payload.as_slice());
    Ok(out)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<String>, CassieError> {
    if bytes.len() > MAX_CHUNK_BYTES {
        return Err(invalid("column-batch row-ID chunk exceeds limit"));
    }
    let mut reader = Reader::new(bytes);
    reader.expect_magic(*MAGIC)?;
    if reader.read_u16()? != FORMAT_VERSION {
        return Err(invalid("unsupported column-batch row-ID version"));
    }
    let count = usize::try_from(reader.read_u32()?)
        .map_err(|_| invalid("column-batch row-ID count overflow"))?;
    if count > MAX_ROW_IDS {
        return Err(invalid("column-batch row-ID count exceeds limit"));
    }
    let payload_len = usize::try_from(reader.read_u32()?)
        .map_err(|_| invalid("column-batch row-ID payload length overflow"))?;
    if payload_len > MAX_CHUNK_BYTES || payload_len != reader.remaining().saturating_sub(8) {
        return Err(invalid("invalid column-batch row-ID payload length"));
    }
    let decoded_len = usize::try_from(reader.read_u64()?)
        .map_err(|_| invalid("column-batch row-ID decoded length overflow"))?;
    if decoded_len > MAX_CHUNK_BYTES || payload_len != reader.remaining() {
        return Err(invalid("invalid column-batch row-ID payload framing"));
    }
    let mut row_ids = Vec::with_capacity(count);
    let mut observed_decoded_len = 0usize;
    for _ in 0..count {
        let row_id = reader.read_bounded_string(MAX_ROW_ID_BYTES)?;
        observed_decoded_len = observed_decoded_len
            .checked_add(row_id.len())
            .ok_or_else(|| invalid("column-batch row-ID decoded length overflow"))?;
        row_ids.push(row_id);
    }
    reader.finish()?;
    if observed_decoded_len != decoded_len {
        return Err(invalid("column-batch row-ID decoded length mismatch"));
    }
    Ok(row_ids)
}
