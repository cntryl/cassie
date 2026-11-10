//! Checked len-equivalent semantic-key estimates for aggregate accounting.
use super::{SemanticKey, SemanticValue};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
thread_local! {
    static NEXT_FRAME_CANCEL: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn cancel_next_estimate_frame(cancel: crate::runtime::QueryCancellationHandle) {
    NEXT_FRAME_CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel));
}

#[cfg(test)]
#[path = "estimated_controlled_tests.rs"]
mod tests;

enum Frame<'a> {
    Value(&'a SemanticValue),
    Values(std::slice::Iter<'a, SemanticValue>),
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
        self.push_observing(frame, || {})
    }

    fn push_observing(
        &mut self,
        frame: Frame<'a>,
        mut after_move: impl FnMut(),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.frames.len() == self.frames.capacity() {
            let capacity = self
                .frames
                .len()
                .checked_add(1)
                .and_then(usize::checked_next_power_of_two)
                .ok_or_else(overflow)?;
            let bytes = capacity
                .checked_mul(std::mem::size_of::<Frame<'_>>())
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
        #[cfg(test)]
        NEXT_FRAME_CANCEL.with(|slot| {
            if let Some(cancel) = slot.borrow_mut().take() {
                cancel.cancel();
            }
        });
        Ok(())
    }
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("semantic group estimate overflow".into())
}

fn checked_root_add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(overflow)
}

impl SemanticKey {
    pub(crate) fn estimated_bytes_controlled(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        let mut stack = Stack::new(controls)?;
        let mut total = 0_usize;
        for value in &self.0 {
            controls.check_interruption()?;
            let mut current = Some(Frame::Value(value));
            let mut value_bytes = 0_usize;
            while let Some(frame) = current.take().or_else(|| stack.frames.pop()) {
                controls.check_interruption()?;
                match frame {
                    Frame::Value(value) => match value {
                        SemanticValue::String(text) | SemanticValue::Json(text) => {
                            value_bytes = value_bytes.saturating_add(text.len());
                        }
                        SemanticValue::Vector(values) => {
                            value_bytes = value_bytes.saturating_add(
                                values.len().saturating_mul(std::mem::size_of::<u32>()),
                            );
                        }
                        SemanticValue::Array(values) => {
                            value_bytes = value_bytes.saturating_add(
                                values
                                    .len()
                                    .saturating_mul(std::mem::size_of::<SemanticValue>()),
                            );
                            stack.push(Frame::Values(values.iter()))?;
                        }
                        SemanticValue::SqlArray {
                            dimensions,
                            elements,
                        } => {
                            value_bytes = value_bytes
                                .saturating_add(
                                    dimensions
                                        .len()
                                        .saturating_mul(std::mem::size_of::<(i32, i32)>()),
                                )
                                .saturating_add(
                                    elements
                                        .len()
                                        .saturating_mul(std::mem::size_of::<SemanticValue>()),
                                );
                            stack.push(Frame::Values(elements.iter()))?;
                        }
                        SemanticValue::Null
                        | SemanticValue::Bool(_)
                        | SemanticValue::Number(_)
                        | SemanticValue::Typed(_)
                        | SemanticValue::Jsonb(_)
                        | SemanticValue::JsonbArray(_)
                        | SemanticValue::Record(_) => {}
                    },
                    Frame::Values(mut values) => {
                        if let Some(value) = values.next() {
                            stack.push(Frame::Values(values))?;
                            current = Some(Frame::Value(value));
                        }
                    }
                }
            }
            total = checked_root_add(total, value_bytes)?;
        }
        Ok(total.saturating_add(
            self.0
                .len()
                .saturating_mul(std::mem::size_of::<SemanticValue>()),
        ))
    }
}
