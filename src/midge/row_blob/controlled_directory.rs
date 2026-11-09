//! Test-only checked directory and borrowed TYPE_JSON span.
use std::cmp::Ordering;
use std::mem::size_of;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::DataType;

use super::super::{
    expected_type_tag, field_bit, Cursor, RowFieldMeta, FORMAT_VERSION, MAGIC, TYPE_JSON,
};

#[derive(Clone, Copy)]
pub(super) enum Progress {
    IdSortMove,
    RangeSortMove,
}

#[derive(Clone, Copy)]
struct Entry {
    id: u32,
    tag: u8,
    offset: usize,
    length: usize,
}

struct Buffer {
    values: Vec<Entry>,
    _memory: QueryMemoryReservation,
}

impl Buffer {
    fn new(capacity: usize, controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        let bytes = capacity
            .checked_mul(size_of::<Entry>())
            .ok_or_else(overflow)?;
        let mut result = Self {
            values: Vec::new(),
            _memory: controls.reserve_query_memory(bytes)?,
        };
        result
            .values
            .try_reserve_exact(capacity)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if result.values.capacity() > capacity {
            return Err(overflow());
        }
        Ok(result)
    }
}

pub(super) struct Directory<'a> {
    presence: &'a [u8],
    nulls: &'a [u8],
    payload: &'a [u8],
    entries: Buffer,
}

#[derive(Debug)]
pub(super) enum HandoffError {
    Directory,
    Utf8,
    Cell,
}

impl<'a> Directory<'a> {
    pub(super) fn parse(
        row: &'a [u8],
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        Self::parse_observing(row, controls, |_| {})
    }

    pub(super) fn parse_observing(
        row: &'a [u8],
        controls: &QueryExecutionControls,
        mut observe: impl FnMut(Progress),
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let mut cursor = Cursor::new(row);
        cursor.expect_bytes(MAGIC)?;
        let version = cursor.read_u8()?;
        if version != FORMAT_VERSION {
            return Err(CassieError::Parse(format!(
                "unsupported row blob format version {version}"
            )));
        }
        let _schema_version = cursor.read_u32()?;
        let _max_field_id = cursor.read_u32()?;
        let bitmap_len = usize::try_from(cursor.read_u32()?)
            .map_err(|_| CassieError::Parse("row blob bitmap length overflow".to_string()))?;
        let presence = cursor.read_exact(bitmap_len)?;
        let nulls = cursor.read_exact(bitmap_len)?;
        let field_count = cursor.read_u32()?;
        let capacity = usize::try_from(field_count).map_err(|_| overflow())?;
        let mut entries = Buffer::new(capacity, controls)?;
        for _ in 0..field_count {
            controls.check_interruption()?;
            let id = cursor.read_u32()?;
            let tag = cursor.read_u8()?;
            let offset = usize::try_from(cursor.read_u32()?)
                .map_err(|_| CassieError::Parse("row blob field offset overflow".to_string()))?;
            let length = usize::try_from(cursor.read_u32()?)
                .map_err(|_| CassieError::Parse("row blob field length overflow".to_string()))?;
            entries.values.push(Entry {
                id,
                tag,
                offset,
                length,
            });
        }
        let payload = cursor.remaining();
        let scratch = Buffer::new(capacity, controls)?;
        let (mut entries, mut ranges) = sort(
            entries,
            scratch,
            |a, b| a.id.cmp(&b.id),
            controls,
            Progress::IdSortMove,
            &mut observe,
        )?;
        let mut write = 0;
        for read in 0..entries.values.len() {
            controls.check_interruption()?;
            if read + 1 < entries.values.len()
                && entries.values[read].id == entries.values[read + 1].id
            {
                continue;
            }
            entries.values[write] = entries.values[read];
            write += 1;
        }
        entries.values.truncate(write);
        ranges.values.clear();
        for entry in &entries.values {
            controls.check_interruption()?;
            let end = entry.offset.checked_add(entry.length).ok_or_else(bounds)?;
            if end > payload.len() {
                return Err(bounds());
            }
            ranges.values.push(*entry);
        }
        let scratch = Buffer::new(capacity, controls)?;
        let (ranges, _scratch) = sort(
            ranges,
            scratch,
            |a, b| (a.offset, a.offset + a.length).cmp(&(b.offset, b.offset + b.length)),
            controls,
            Progress::RangeSortMove,
            &mut observe,
        )?;
        let mut covered = 0;
        for entry in &ranges.values {
            controls.check_interruption()?;
            if entry.offset != covered {
                return Err(CassieError::Parse(
                    "row blob field ranges must partition the payload".to_string(),
                ));
            }
            covered = entry.offset + entry.length;
        }
        if covered != payload.len() {
            return Err(CassieError::Parse(
                "row blob contains trailing bytes".to_string(),
            ));
        }
        Ok(Self {
            presence,
            nulls,
            payload,
            entries,
        })
    }

    pub(super) fn json_span(
        &self,
        field: &RowFieldMeta,
        controls: &QueryExecutionControls,
    ) -> Result<Option<&'a [u8]>, CassieError> {
        if !self.entries._memory.belongs_to(controls) {
            return Err(CassieError::ResourceLimit(
                "row directory uses a different query tracker".into(),
            ));
        }
        if !field_bit(self.presence, field.field_id) || field_bit(self.nulls, field.field_id) {
            return Ok(None);
        }
        if !matches!(field.data_type, DataType::Json | DataType::Jsonb) {
            return Err(CassieError::Execution(
                "raw JSON span requires a JSON field".into(),
            ));
        }
        let mut left = 0;
        let mut right = self.entries.values.len();
        while left < right {
            controls.check_interruption()?;
            let mid = left + (right - left) / 2;
            match self.entries.values[mid].id.cmp(&field.field_id) {
                Ordering::Less => left = mid + 1,
                Ordering::Greater => right = mid,
                Ordering::Equal => {
                    let entry = self.entries.values[mid];
                    let expected = expected_type_tag(&field.data_type);
                    if entry.tag != expected {
                        return Err(CassieError::Parse(format!("row blob field '{}' stores type tag {} but its schema declares type tag {expected}", field.name, entry.tag)));
                    }
                    debug_assert_eq!(entry.tag, TYPE_JSON);
                    return Ok(Some(
                        &self.payload[entry.offset..entry.offset + entry.length],
                    ));
                }
            }
        }
        Err(CassieError::Parse(
            "row blob field missing directory entry".to_string(),
        ))
    }

    pub(super) fn json_cell(
        &self,
        field: &RowFieldMeta,
        controls: &QueryExecutionControls,
    ) -> Result<Option<crate::runtime::accounted::json_arena::PlainCell>, HandoffError> {
        self.json_cell_observing(field, controls, || {})
    }

    pub(super) fn json_cell_observing(
        &self,
        field: &RowFieldMeta,
        controls: &QueryExecutionControls,
        copied: impl FnMut(),
    ) -> Result<Option<crate::runtime::accounted::json_arena::PlainCell>, HandoffError> {
        use crate::runtime::accounted::json_arena::{PlainCell, PlainIdentity};
        let Some(bytes) = self
            .json_span(field, controls)
            .map_err(|_| HandoffError::Directory)?
        else {
            return Ok(None);
        };
        let identity = match field.data_type {
            DataType::Json => PlainIdentity::Json,
            DataType::Jsonb => PlainIdentity::Jsonb,
            _ => return Err(HandoffError::Directory),
        };
        PlainCell::parse_bytes_observing(bytes, identity, controls, copied)
            .map(Some)
            .map_err(|error| match error {
                crate::runtime::accounted::json_arena::PlainCellError::Utf8 => HandoffError::Utf8,
                _ => HandoffError::Cell,
            })
    }
}

fn sort(
    mut source: Buffer,
    mut scratch: Buffer,
    compare: impl Fn(&Entry, &Entry) -> Ordering,
    controls: &QueryExecutionControls,
    progress: Progress,
    observe: &mut impl FnMut(Progress),
) -> Result<(Buffer, Buffer), CassieError> {
    let length = source.values.len();
    let mut width = 1_usize;
    while width < length {
        scratch.values.clear();
        let mut start = 0;
        while start < length {
            let middle = start.saturating_add(width).min(length);
            let end = middle.saturating_add(width).min(length);
            let mut left = start;
            let mut right = middle;
            while left < middle || right < end {
                controls.check_interruption()?;
                let take_left = right == end
                    || (left < middle
                        && compare(&source.values[left], &source.values[right])
                            != Ordering::Greater);
                let index = if take_left {
                    let index = left;
                    left += 1;
                    index
                } else {
                    let index = right;
                    right += 1;
                    index
                };
                scratch.values.push(source.values[index]);
                observe(progress);
                controls.check_interruption()?;
            }
            start = end;
        }
        std::mem::swap(&mut source, &mut scratch);
        width = width.saturating_mul(2);
    }
    Ok((source, scratch))
}

fn bounds() -> CassieError {
    CassieError::Parse("invalid row blob field bounds".to_string())
}
fn overflow() -> CassieError {
    CassieError::ResourceLimit("controlled row directory capacity overflow".into())
}
