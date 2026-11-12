//! Isolated row-cell-to-DataRow experiment; not wired to a result or socket.
use crate::app::CassieError;
use crate::runtime::accounted::json_arena::{PlainCell, PlainIdentity};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[path = "raw_json_frame_tests.rs"]
mod tests;

struct RawFrame {
    bytes: Vec<u8>,
    _memory: QueryMemoryReservation,
}

impl RawFrame {
    fn build(
        cell: Option<&PlainCell>,
        oid: i64,
        format: i16,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        Self::build_observing(cell, oid, format, controls, || {})
    }

    fn build_observing(
        cell: Option<&PlainCell>,
        oid: i64,
        format: i16,
        controls: &QueryExecutionControls,
        mut copied: impl FnMut(),
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        if !matches!(oid, 114 | 3802) || !matches!(format, 0 | 1) {
            return Err(invalid());
        }
        if let Some(value) = cell {
            let expected = if oid == 114 {
                PlainIdentity::Json
            } else {
                PlainIdentity::Jsonb
            };
            if value.identity() != expected || !value.belongs_to(controls) {
                return Err(invalid());
            }
        }
        let version = cell.is_some() && oid == 3802 && format == 1;
        let (field_len, total) =
            checked_lengths(cell.map_or(0, |value| value.text().len()), version)?;
        let wire_len = i32::try_from(total - 1).map_err(|_| invalid())?;
        let mut frame = Self {
            bytes: Vec::new(),
            _memory: controls.reserve_query_memory(total)?,
        };
        frame
            .bytes
            .try_reserve_exact(total)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if frame.bytes.capacity() > total {
            return Err(invalid());
        }
        frame.bytes.push(b'D');
        frame.bytes.extend_from_slice(&wire_len.to_be_bytes());
        frame.bytes.extend_from_slice(&1_i16.to_be_bytes());
        frame
            .bytes
            .extend_from_slice(&cell.map_or(-1, |_| field_len).to_be_bytes());
        if version {
            frame.bytes.push(1);
        }
        if let Some(value) = cell {
            for chunk in value.text().as_bytes().chunks(1024) {
                controls.check_interruption()?;
                frame.bytes.extend_from_slice(chunk);
                copied();
                controls.check_interruption()?;
            }
        }
        Ok(frame)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

fn checked_lengths(text_len: usize, version: bool) -> Result<(i32, usize), CassieError> {
    let payload = text_len
        .checked_add(usize::from(version))
        .ok_or_else(invalid)?;
    let field_len = i32::try_from(payload).map_err(|_| invalid())?;
    let total = payload.checked_add(11).ok_or_else(invalid)?;
    let _ = i32::try_from(total - 1).map_err(|_| invalid())?;
    Ok((field_len, total))
}

fn invalid() -> CassieError {
    CassieError::ResourceLimit("invalid raw JSON frame".into())
}
