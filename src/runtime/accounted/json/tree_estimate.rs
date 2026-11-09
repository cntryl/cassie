//! Checked iterative equivalent of the existing retained JSON estimate.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use serde_json::Value;
use std::mem::size_of;

#[cfg(test)]
#[path = "tree_estimate_tests.rs"]
mod tests;

#[cfg(test)]
thread_local! {
    static CANCEL_MOVE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) fn cancel_move_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_MOVE.with(|slot| *slot.borrow_mut() = Some(handle));
}

enum Frame<'a> {
    Value(&'a Value),
    Array(std::slice::Iter<'a, Value>),
    Object(serde_json::map::Iter<'a>),
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
    fn new(controls: &'a QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            frames: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
            controls,
        })
    }
    fn push(&mut self, frame: Frame<'a>) -> Result<(), CassieError> {
        if self.frames.len() == self.frames.capacity() {
            let capacity = self
                .frames
                .len()
                .checked_mul(2)
                .unwrap_or(usize::MAX)
                .max(4);
            let bytes = capacity
                .checked_mul(size_of::<Frame<'a>>())
                .ok_or_else(overflow)?;
            let mut replacement = Replacement {
                frames: Vec::new(),
                memory: self.controls.reserve_query_memory(bytes)?,
            };
            replacement
                .frames
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.frames.capacity() > capacity {
                return Err(overflow());
            }
            while let Some(frame) = self.frames.pop() {
                self.controls.check_interruption()?;
                replacement.frames.push(frame);
                #[cfg(test)]
                CANCEL_MOVE.with(|slot| {
                    if let Some(handle) = slot.borrow_mut().take() {
                        handle.cancel();
                    }
                });
                self.controls.check_interruption()?;
            }
            let mut left = 0;
            let mut right = replacement.frames.len();
            while left < right {
                self.controls.check_interruption()?;
                right -= 1;
                if left >= right {
                    break;
                }
                replacement.frames.swap(left, right);
                left += 1;
            }
            let Replacement { frames, memory } = replacement;
            let old_frames = std::mem::replace(&mut self.frames, frames);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old_frames);
            drop(old_memory);
        }
        self.frames.push(frame);
        Ok(())
    }
}

pub(crate) fn estimate(
    value: &Value,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    controls.check_interruption()?;
    let mut total = size_of::<Value>();
    let mut stack = Stack::new(controls)?;
    stack.push(Frame::Value(value))?;
    while let Some(frame) = stack.frames.pop() {
        controls.check_interruption()?;
        match frame {
            Frame::Value(Value::Null | Value::Bool(_)) => {}
            Frame::Value(Value::String(text)) => total = overflow_sum(total, text.capacity())?,
            Frame::Value(Value::Number(number)) => {
                let bytes = overflow_sum(checked_mul(number.as_str().len(), 2)?, 64)?;
                total = overflow_sum(total, bytes)?;
            }
            Frame::Value(Value::Array(values)) => {
                total = overflow_sum(total, checked_mul(values.capacity(), size_of::<Value>())?)?;
                stack.push(Frame::Array(values.iter()))?;
            }
            Frame::Value(Value::Object(values)) => {
                total = overflow_sum(
                    total,
                    checked_mul(values.len().max(1), super::JSON_MAP_ENTRY_BYTES)?,
                )?;
                stack.push(Frame::Object(values.iter()))?;
            }
            Frame::Array(mut iter) => {
                if let Some(value) = iter.next() {
                    stack.push(Frame::Array(iter))?;
                    stack.push(Frame::Value(value))?;
                }
            }
            Frame::Object(mut iter) => {
                if let Some((key, value)) = iter.next() {
                    total = overflow_sum(total, key.capacity())?;
                    stack.push(Frame::Object(iter))?;
                    stack.push(Frame::Value(value))?;
                }
            }
        }
    }
    Ok(total)
}

fn overflow_sum(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(overflow)
}
fn checked_mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(overflow)
}
fn overflow() -> CassieError {
    CassieError::ResourceLimit("retained JSON accounting overflow".into())
}
