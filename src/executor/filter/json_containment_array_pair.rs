//! Owned raw array sources and admitted child ranges for the containment walk.
use super::render::{self, RenderError};
use crate::app::CassieError;
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use serde_json::value::RawValue;
use std::ops::Range;

#[cfg(test)]
thread_local! {
    static CANCEL_MOVE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(in crate::executor::filter) fn cancel_move_once(
    handle: crate::runtime::QueryCancellationHandle,
) {
    CANCEL_MOVE.with(|slot| *slot.borrow_mut() = Some(handle));
}

struct Directory {
    ranges: Vec<Range<usize>>,
    memory: QueryMemoryReservation,
}
struct Replacement {
    ranges: Vec<Range<usize>>,
    memory: QueryMemoryReservation,
}

impl Directory {
    fn new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self {
            ranges: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }
    fn push(
        &mut self,
        value: Range<usize>,
        controls: &QueryExecutionControls,
    ) -> Result<(), QueryError> {
        if self.ranges.len() == self.ranges.capacity() {
            let capacity = self
                .ranges
                .len()
                .checked_mul(2)
                .unwrap_or(usize::MAX)
                .max(4);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<Range<usize>>())
                .ok_or_else(overflow)?;
            let mut replacement = Replacement {
                ranges: Vec::new(),
                memory: controls.reserve_query_memory(bytes)?,
            };
            replacement
                .ranges
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.ranges.capacity() > capacity {
                return Err(overflow().into());
            }
            while let Some(item) = self.ranges.pop() {
                controls.check_interruption()?;
                replacement.ranges.push(item);
                #[cfg(test)]
                CANCEL_MOVE.with(|slot| {
                    if let Some(handle) = slot.borrow_mut().take() {
                        handle.cancel();
                    }
                });
                controls.check_interruption()?;
            }
            let mut left = 0;
            let mut right = replacement.ranges.len();
            while left < right {
                controls.check_interruption()?;
                right -= 1;
                if left >= right {
                    break;
                }
                replacement.ranges.swap(left, right);
                left += 1;
            }
            let Replacement { ranges, memory } = replacement;
            let old_ranges = std::mem::replace(&mut self.ranges, ranges);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old_ranges);
            drop(old_memory);
        }
        self.ranges.push(value);
        Ok(())
    }
}

pub(in crate::executor::filter) struct ArrayPair {
    candidate: Box<RawValue>,
    required: Box<RawValue>,
    candidate_ranges: Directory,
    required_ranges: Directory,
    candidate_index: usize,
    required_index: usize,
}

pub(in crate::executor::filter) enum ArrayStep<'a> {
    Child(&'a str, &'a str),
    Missing,
    Done,
}

impl ArrayPair {
    pub(in crate::executor::filter) fn build(
        candidate: Box<RawValue>,
        required: Box<RawValue>,
        required_is_array: bool,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        let required_ranges = if required_is_array {
            parse(required.get(), controls)?
        } else {
            let mut ranges = Directory::new(controls)?;
            ranges.push(0..required.get().len(), controls)?;
            ranges
        };
        let candidate_ranges = parse(candidate.get(), controls)?;
        Ok(Self {
            candidate,
            required,
            candidate_ranges,
            required_ranges,
            candidate_index: 0,
            required_index: 0,
        })
    }

    pub(in crate::executor::filter) fn next(
        &mut self,
        previous: Option<bool>,
        controls: &QueryExecutionControls,
    ) -> Result<ArrayStep<'_>, QueryError> {
        controls.check_interruption()?;
        if previous == Some(true) {
            self.required_index += 1;
            self.candidate_index = 0;
        }
        let Some(required) = self.required_ranges.ranges.get(self.required_index) else {
            return Ok(ArrayStep::Done);
        };
        let Some(candidate) = self.candidate_ranges.ranges.get(self.candidate_index) else {
            return Ok(ArrayStep::Missing);
        };
        self.candidate_index += 1;
        Ok(ArrayStep::Child(
            &self.candidate.get()[candidate.clone()],
            &self.required.get()[required.clone()],
        ))
    }
}

fn parse(source: &str, controls: &QueryExecutionControls) -> Result<Directory, QueryError> {
    let mut result = Directory::new(controls)?;
    let mut scanner =
        render::array_scanner(source, controls).map_err(RenderError::into_query_error)?;
    let _ = scanner
        .next()
        .map_err(RenderError::from)
        .map_err(RenderError::into_query_error)?;
    loop {
        scanner
            .whitespace()
            .map_err(RenderError::from)
            .map_err(RenderError::into_query_error)?;
        if scanner.peek() == Some(b']') {
            let _ = scanner
                .next()
                .map_err(RenderError::from)
                .map_err(RenderError::into_query_error)?;
            break;
        }
        if scanner.peek() == Some(b',') {
            let _ = scanner
                .next()
                .map_err(RenderError::from)
                .map_err(RenderError::into_query_error)?;
            scanner
                .whitespace()
                .map_err(RenderError::from)
                .map_err(RenderError::into_query_error)?;
        }
        let span = scanner
            .value_span()
            .map_err(RenderError::from)
            .map_err(RenderError::into_query_error)?;
        result.push(span, controls)?;
    }
    Ok(result)
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("JSON array directory overflow".into())
}
