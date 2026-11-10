//! Isolated binary JSONB-array envelope with ordered element validation.
use std::ops::Range;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "jsonb_array_envelope_tests.rs"]
mod tests;

#[derive(Clone, Copy, Default)]
struct Dimension {
    length: i32,
    lower: i32,
}

#[derive(Debug)]
pub(crate) enum DecodeError<E> {
    InvalidArray,
    DocumentNull,
    Element(E),
    Control(CassieError),
}

#[derive(Clone, Copy)]
pub(crate) enum Progress {
    Element,
}

pub(crate) struct Envelope<'a> {
    source: &'a [u8],
    dimensions: [Dimension; 6],
    count: usize,
    elements: Vec<Option<Range<usize>>>,
    _memory: QueryMemoryReservation,
    compact_fallback: bool,
}

pub(super) enum Element<'a> {
    SqlNull,
    Value(&'a [u8]),
}

impl<'a> Envelope<'a> {
    pub(super) fn compact_fallback(&self) -> bool {
        self.compact_fallback
    }
    pub(super) fn dimensions(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.dimensions[..self.count]
            .iter()
            .map(|dimension| (dimension.length, dimension.lower))
    }
    pub(super) fn element(&self, position: usize) -> Option<Element<'a>> {
        self.elements.get(position).map(|range| match range {
            None => Element::SqlNull,
            Some(range) => Element::Value(&self.source[range.clone()]),
        })
    }
}

pub(crate) fn decode<'a, E>(
    source: &'a [u8],
    controls: &QueryExecutionControls,
    mut validate: impl FnMut(&[u8]) -> Result<bool, E>,
    mut observer: impl FnMut(Progress),
) -> Result<Envelope<'a>, DecodeError<E>> {
    controls
        .check_interruption()
        .map_err(DecodeError::Control)?;
    let mut cursor = 0_usize;
    let count = read_i32(source, &mut cursor).ok_or(DecodeError::InvalidArray)?;
    let null_flag = read_i32(source, &mut cursor).ok_or(DecodeError::InvalidArray)?;
    let oid = read_i32(source, &mut cursor).ok_or(DecodeError::InvalidArray)?;
    if !(0..=6).contains(&count) || !matches!(null_flag, 0 | 1) || oid != 3802 {
        return Err(DecodeError::InvalidArray);
    }
    let count = usize::try_from(count).map_err(|_| DecodeError::InvalidArray)?;
    if count == 0 && (null_flag != 0 || cursor != source.len()) {
        return Err(DecodeError::InvalidArray);
    }
    let mut dimensions = [Dimension::default(); 6];
    let mut element_count = usize::from(count != 0);
    for dimension in &mut dimensions[..count] {
        controls
            .check_interruption()
            .map_err(DecodeError::Control)?;
        let length = read_i32(source, &mut cursor).ok_or(DecodeError::InvalidArray)?;
        let lower = read_i32(source, &mut cursor).ok_or(DecodeError::InvalidArray)?;
        element_count = element_count
            .checked_mul(usize::try_from(length).map_err(|_| DecodeError::InvalidArray)?)
            .ok_or(DecodeError::InvalidArray)?;
        *dimension = Dimension { length, lower };
    }
    let minimum = element_count
        .checked_mul(std::mem::size_of::<i32>())
        .ok_or(DecodeError::InvalidArray)?;
    if source.len().saturating_sub(cursor) < minimum {
        return Err(DecodeError::InvalidArray);
    }
    let empty = controls
        .reserve_query_memory(0)
        .map_err(DecodeError::Control)?;
    let memory;
    let mut elements;
    (elements, memory) = crate::runtime::owned_memory::try_reserve_vec(element_count, &empty)
        .map_err(DecodeError::Control)?;
    let mut sql_null = false;
    let mut document_null = false;
    for _ in 0..element_count {
        controls
            .check_interruption()
            .map_err(DecodeError::Control)?;
        let length = read_i32(source, &mut cursor).ok_or(DecodeError::InvalidArray)?;
        if length == -1 {
            sql_null = true;
            elements.push(None);
            continue;
        }
        let length = usize::try_from(length).map_err(|_| DecodeError::InvalidArray)?;
        let end = cursor
            .checked_add(length)
            .ok_or(DecodeError::InvalidArray)?;
        let field = source.get(cursor..end).ok_or(DecodeError::InvalidArray)?;
        let is_document_null = validate(field).map_err(DecodeError::Element)?;
        observer(Progress::Element);
        controls
            .check_interruption()
            .map_err(DecodeError::Control)?;
        document_null |= is_document_null;
        elements.push(Some(cursor..end));
        cursor = end;
    }
    if cursor != source.len() || (null_flag == 0 && sql_null) {
        return Err(DecodeError::InvalidArray);
    }
    if document_null {
        return Err(DecodeError::DocumentNull);
    }
    for dimension in &dimensions[..count] {
        controls
            .check_interruption()
            .map_err(DecodeError::Control)?;
        if dimension.length <= 0 || dimension.lower.checked_add(dimension.length - 1).is_none() {
            return Err(DecodeError::InvalidArray);
        }
    }
    let nonstandard = dimensions[..count]
        .iter()
        .any(|dimension| dimension.lower != 1);
    let compact_fallback = nonstandard
        && dimensions[..count].iter().any(|dimension| {
            dimension
                .lower
                .checked_add(dimension.length)
                .and_then(|upper| upper.checked_sub(1))
                .is_none()
        });
    Ok(Envelope {
        source,
        dimensions,
        count,
        elements,
        _memory: memory,
        compact_fallback,
    })
}

fn read_i32(source: &[u8], cursor: &mut usize) -> Option<i32> {
    let end = cursor.checked_add(4)?;
    let bytes: [u8; 4] = source.get(*cursor..end)?.try_into().ok()?;
    *cursor = end;
    Some(i32::from_be_bytes(bytes))
}
