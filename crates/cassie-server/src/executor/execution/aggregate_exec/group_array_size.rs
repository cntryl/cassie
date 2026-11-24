//! Checked size counting for derived serde SQL Array values.
use crate::app::CassieError;
use crate::executor::retained_memory::mul;
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{SqlArray, Value};

use super::{non_array_value_bytes, scalar_json_bytes, supported_typed};

#[cfg(test)]
#[path = "group_array_size_tests.rs"]
mod tests;

pub(super) enum ArraySize {
    Bytes(usize),
    Unsupported,
    SerializationFailed,
}

enum Frame<'a> {
    Array(&'a SqlArray),
    Elements(std::slice::Iter<'a, Value>),
}

struct Stack<'a> {
    frames: Vec<Frame<'a>>,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
}

struct Replacement<'a> {
    frames: Vec<Frame<'a>>,
    memory: QueryMemoryReservation,
}

impl<'a> Stack<'a> {
    fn new(controls: &'a QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self {
            frames: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
            controls,
        })
    }

    fn push(&mut self, frame: Frame<'a>) -> Result<(), QueryError> {
        self.push_observing(frame, || {})
    }

    fn push_observing(
        &mut self,
        frame: Frame<'a>,
        mut after_move: impl FnMut(),
    ) -> Result<(), QueryError> {
        self.controls.check_interruption()?;
        if self.frames.len() == self.frames.capacity() {
            let capacity = self
                .frames
                .len()
                .checked_add(1)
                .and_then(usize::checked_next_power_of_two)
                .ok_or_else(overflow)?;
            let mut replacement = Replacement {
                frames: Vec::new(),
                memory: self
                    .controls
                    .reserve_query_memory(mul(capacity, std::mem::size_of::<Frame<'_>>())?)?,
            };
            replacement
                .frames
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.frames.capacity() > capacity {
                return Err(overflow().into());
            }
            while let Some(item) = self.frames.pop() {
                self.controls.check_interruption()?;
                replacement.frames.push(item);
                after_move();
                self.controls.check_interruption()?;
            }
            let len = replacement.frames.len();
            for index in 0..len / 2 {
                self.controls.check_interruption()?;
                replacement.frames.swap(index, len - index - 1);
            }
            self.controls.check_interruption()?;
            let old = std::mem::replace(&mut self.frames, replacement.frames);
            let old_memory = std::mem::replace(&mut self.memory, replacement.memory);
            drop(old);
            drop(old_memory);
        }
        self.frames.push(frame);
        Ok(())
    }
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("aggregate array size stack exceeded its admitted capacity".into())
}

pub(super) fn count<'a>(
    array: &'a SqlArray,
    controls: &'a QueryExecutionControls,
    after_chunk: &mut impl FnMut(),
) -> Result<ArraySize, QueryError> {
    let mut stack = Stack::new(controls)?;
    stack.push(Frame::Array(array))?;
    let mut bytes = 0_usize;
    let mut steps = 0_usize;
    while let Some(frame) = stack.frames.pop() {
        controls.check_interruption()?;
        match frame {
            Frame::Array(array) => {
                bytes = bytes.saturating_add("{\"Array\":{\"dimensions\":[".len());
                for (index, dimension) in array.dimensions().iter().enumerate() {
                    controls.check_interruption()?;
                    let (Some(length), Some(lower_bound)) = (
                        scalar_json_bytes(&dimension.length),
                        scalar_json_bytes(&dimension.lower_bound),
                    ) else {
                        return Ok(ArraySize::SerializationFailed);
                    };
                    bytes = bytes
                        .saturating_add(usize::from(index != 0))
                        .saturating_add("{\"length\":".len())
                        .saturating_add(length)
                        .saturating_add(",\"lower_bound\":".len())
                        .saturating_add(lower_bound)
                        .saturating_add(1);
                    steps = steps.wrapping_add(1);
                    if steps.is_multiple_of(256) {
                        after_chunk();
                        controls.check_interruption()?;
                    }
                }
                bytes = bytes
                    .saturating_add("],\"elements\":[".len())
                    .saturating_add(array.elements().len().saturating_sub(1))
                    .saturating_add("]}}".len());
                stack.push(Frame::Elements(array.elements().iter()))?;
            }
            Frame::Elements(mut elements) => {
                if let Some(value) = elements.next() {
                    stack.push(Frame::Elements(elements))?;
                    steps = steps.wrapping_add(1);
                    if steps.is_multiple_of(256) {
                        after_chunk();
                        controls.check_interruption()?;
                    }
                    match value {
                        Value::Array(array) => stack.push(Frame::Array(array))?,
                        Value::Typed(typed) if !supported_typed(typed) => {
                            return Ok(ArraySize::Unsupported);
                        }
                        _ => match non_array_value_bytes(value, controls, after_chunk)? {
                            Some(size) => bytes = bytes.saturating_add(size),
                            None => return Ok(ArraySize::SerializationFailed),
                        },
                    }
                }
            }
        }
    }
    Ok(ArraySize::Bytes(bytes))
}
