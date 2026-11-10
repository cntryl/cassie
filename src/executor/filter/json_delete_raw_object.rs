//! Reached object keys and borrowed raw child spans for JSON deletion.
use std::fmt;
use std::ops::Range;

use crate::app::CassieError;
use crate::runtime::accounted::json_arena::raw_decode::{self, DecodeError, RawPosition};
use crate::runtime::accounted::Accounted;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{ScanError, Scanner};

#[cfg(test)]
#[path = "json_delete_raw_object_tests.rs"]
mod tests;

#[path = "json_delete_raw_object_order.rs"]
mod order;

#[derive(Debug)]
pub(in crate::executor::filter::json_delete) enum ObjectError {
    Control(CassieError),
    Scan(ScanError),
    Decode(DecodeError),
}

impl fmt::Display for ObjectError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(error) => write!(out, "{error}"),
            Self::Scan(error) => write!(out, "{error}"),
            Self::Decode(error) => write!(out, "{error}"),
        }
    }
}

impl From<CassieError> for ObjectError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

impl From<ScanError> for ObjectError {
    fn from(error: ScanError) -> Self {
        match error {
            ScanError::Control(error) => Self::Control(error),
            other @ ScanError::Syntax { .. } => Self::Scan(other),
        }
    }
}

#[derive(Clone, Copy)]
enum Progress {
    Entry,
    DirectoryMove,
}

struct Field {
    key: Accounted<String>,
    value: Range<usize>,
}

pub(super) struct Fields<'a> {
    source: &'a str,
    entries: Vec<Field>,
    memory: QueryMemoryReservation,
}

struct Replacement {
    entries: Vec<Field>,
    memory: QueryMemoryReservation,
}

pub(super) struct OrderedFields<'a> {
    fields: Fields<'a>,
    indices: Vec<usize>,
    _memory: QueryMemoryReservation,
}

pub(super) struct DetachedFields {
    entries: Vec<Field>,
    indices: Vec<usize>,
    _field_memory: QueryMemoryReservation,
    _index_memory: QueryMemoryReservation,
}

impl DetachedFields {
    pub(super) fn get<'a>(&'a self, source: &'a str, index: usize) -> Option<(&'a str, &'a str)> {
        let field = self.entries.get(*self.indices.get(index)?)?;
        Some((field.key.get().as_str(), &source[field.value.clone()]))
    }
}

impl<'a> OrderedFields<'a> {
    pub(super) fn get(&self, index: usize) -> Option<(&str, &'a str)> {
        let field = self.fields.entries.get(*self.indices.get(index)?)?;
        Some((
            field.key.get().as_str(),
            &self.fields.source[field.value.clone()],
        ))
    }

    pub(super) fn into_detached(self) -> DetachedFields {
        let Self {
            fields,
            indices,
            _memory: index_memory,
        } = self;
        let Fields {
            source: _,
            entries,
            memory: field_memory,
        } = fields;
        DetachedFields {
            entries,
            indices,
            _field_memory: field_memory,
            _index_memory: index_memory,
        }
    }
}

impl<'a> Fields<'a> {
    fn new(source: &'a str, controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            source,
            entries: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    fn push(
        &mut self,
        field: Field,
        controls: &QueryExecutionControls,
        observe: &mut impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        if self.entries.len() == self.entries.capacity() {
            let capacity = self.entries.len().saturating_mul(2).max(4);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<Field>())
                .ok_or_else(|| {
                    CassieError::ResourceLimit("JSON object directory overflow".into())
                })?;
            let memory = controls.reserve_query_memory(bytes)?;
            let mut replacement = Replacement {
                entries: Vec::new(),
                memory,
            };
            replacement
                .entries
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.entries.capacity() > capacity {
                return Err(CassieError::ResourceLimit(
                    "JSON object directory exceeded its admission".into(),
                ));
            }
            while let Some(entry) = self.entries.pop() {
                controls.check_interruption()?;
                replacement.entries.push(entry);
                observe(Progress::DirectoryMove);
                controls.check_interruption()?;
            }
            let mut left = 0;
            let mut right = replacement.entries.len();
            while left < right {
                controls.check_interruption()?;
                right -= 1;
                if left >= right {
                    break;
                }
                replacement.entries.swap(left, right);
                left += 1;
            }
            let Replacement { entries, memory } = replacement;
            let old_entries = std::mem::replace(&mut self.entries, entries);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old_entries);
            drop(old_memory);
        }
        self.entries.push(field);
        observe(Progress::Entry);
        controls.check_interruption()?;
        Ok(())
    }

    pub(super) fn last_value(
        &self,
        key: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Option<&'a str>, CassieError> {
        for field in self.entries.iter().rev() {
            controls.check_interruption()?;
            if equal(field.key.get(), key, controls)? {
                return Ok(Some(&self.source[field.value.clone()]));
            }
        }
        Ok(None)
    }

    fn ordered<'b>(
        &'b self,
        controls: &QueryExecutionControls,
    ) -> Result<order::Order<'b>, CassieError> {
        order::sort(self, controls)
    }

    pub(super) fn into_ordered(
        self,
        controls: &QueryExecutionControls,
    ) -> Result<OrderedFields<'a>, CassieError> {
        let (indices, memory) = self.ordered(controls)?.into_parts();
        Ok(OrderedFields {
            fields: self,
            indices,
            _memory: memory,
        })
    }

    pub(super) fn into_lexical(
        self,
        controls: &QueryExecutionControls,
    ) -> Result<OrderedFields<'a>, CassieError> {
        let (indices, memory) = order::sort_lexical(&self, controls)?.into_parts();
        Ok(OrderedFields {
            fields: self,
            indices,
            _memory: memory,
        })
    }
}

#[cfg(test)]
pub(super) fn cancel_lexical_sort_once(handle: crate::runtime::QueryCancellationHandle) {
    order::cancel_lexical_sort_once(handle);
}

pub(super) fn equal(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left
        .as_bytes()
        .chunks(1024)
        .zip(right.as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn parse<'a>(
    input: &'a str,
    controls: &QueryExecutionControls,
) -> Result<Fields<'a>, ObjectError> {
    parse_observing(input, controls, |_| {})
}

fn parse_observing<'a>(
    input: &'a str,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(Progress),
) -> Result<Fields<'a>, ObjectError> {
    controls.check_interruption()?;
    let mut scanner = Scanner {
        input: input.as_bytes(),
        position: 0,
        line: 1,
        column: 0,
        controls,
        observe: |_| {},
    };
    let mut fields = Fields::new(input, controls)?;
    scanner.whitespace()?;
    if scanner.peek() != Some(b'{') {
        return Err(scanner.error("expected object").into());
    }
    let _consumed = scanner.next()?;
    let mut first = true;
    loop {
        scanner.whitespace()?;
        match scanner.peek() {
            Some(b'}') => {
                let _consumed = scanner.next()?;
                break;
            }
            None => return Err(scanner.error("EOF while parsing an object").into()),
            Some(b',') if !first => {
                let _consumed = scanner.next()?;
                scanner.whitespace()?;
                match scanner.peek() {
                    Some(b'"') => {}
                    Some(b'}') => return Err(scanner.error("trailing comma").into()),
                    None => return Err(scanner.error("EOF while parsing a value").into()),
                    Some(_) => return Err(scanner.error("key must be a string").into()),
                }
            }
            Some(b'"') if first => {}
            Some(_) if first => return Err(scanner.error("key must be a string").into()),
            Some(_) => return Err(scanner.error("expected `,` or `}`").into()),
        }
        first = false;
        let position = RawPosition {
            offset: scanner.position,
            line: scanner.line,
            column: scanner.column,
        };
        let key = raw_decode::decode_at(input, position, controls).map_err(|error| match error
            .into_control()
        {
            Ok(error) => ObjectError::Control(error),
            Err(error) => ObjectError::Decode(error),
        })?;
        scanner.string()?;
        scanner.whitespace()?;
        match scanner.peek() {
            Some(b':') => {
                let _consumed = scanner.next()?;
            }
            None => return Err(scanner.error("EOF while parsing an object").into()),
            Some(_) => return Err(scanner.error("expected `:`").into()),
        }
        let value = scanner.value_span()?;
        fields.push(Field { key, value }, controls, &mut observe)?;
    }
    scanner.whitespace()?;
    if scanner.peek().is_some() {
        return Err(scanner.error("trailing characters").into());
    }
    Ok(fields)
}
