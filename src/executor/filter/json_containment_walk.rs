//! Iterative containment: each live parser frame owns its admitted workspace.

use std::collections::{btree_map, BTreeMap};

use serde_json::value::RawValue;

use super::{json_document, kind, CassieError, JsonKind, QueryError};
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[path = "json_containment_number.rs"]
mod numbers;
#[cfg(test)]
pub(super) use numbers::{cancel_after_number_compare_once, cancel_after_number_format_once};

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER_STRING_COPY: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static CANCEL_AFTER_STRING_COMPARE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn cancel_after_string_copy_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_AFTER_STRING_COPY.with(|slot| *slot.borrow_mut() = Some(handle));
}

#[cfg(test)]
pub(super) fn cancel_after_string_compare_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_AFTER_STRING_COMPARE.with(|slot| *slot.borrow_mut() = Some(handle));
}

struct Pair {
    candidate: Box<RawValue>,
    required: Box<RawValue>,
    memory: Option<QueryMemoryReservation>,
}

struct Work<'a> {
    controls: Option<&'a QueryExecutionControls>,
}

impl Work<'_> {
    fn check(&self) -> Result<(), QueryError> {
        self.controls
            .map_or(Ok(()), QueryExecutionControls::check_interruption)
            .map_err(Into::into)
    }

    fn reserve(&self, bytes: usize) -> Result<Option<QueryMemoryReservation>, QueryError> {
        self.check()?;
        self.controls
            .map(|controls| controls.reserve_query_memory(bytes))
            .transpose()
            .map_err(Into::into)
    }

    fn pair(&self, candidate: &str, required: &str) -> Result<Pair, QueryError> {
        let decode = crate::runtime::accounted::json::serialized_decode_bytes(add(
            candidate.len(),
            required.len(),
        )?)?;
        let memory = self.reserve(add(decode, std::mem::size_of::<Frame>())?)?;
        // RawValue uses an iterative parser without the Value depth limit.
        // Each child pair is admitted independently before either raw copy.
        let candidate = json_document::parse_value(candidate)?;
        let required = json_document::parse_value(required)?;
        self.check()?;
        Ok(Pair {
            candidate,
            required,
            memory,
        })
    }

    fn scalar_equal(&self, candidate: &RawValue, required: &RawValue) -> Result<bool, QueryError> {
        self.check()?;
        if let Some(controls) = self.controls {
            if kind(candidate) == JsonKind::String && kind(required) == JsonKind::String {
                return string_equal(candidate.get(), required.get(), controls);
            }
            if kind(candidate) == JsonKind::Number && kind(required) == JsonKind::Number {
                return numbers::equal(candidate.get(), required.get(), controls);
            }
        }
        let _numbers = if kind(candidate) == JsonKind::Number && kind(required) == JsonKind::Number
        {
            let bound = |value: &RawValue| {
                if let Ok(bound) = crate::types::jsonb::input_text_len(value.get()) {
                    return Ok::<usize, QueryError>(bound);
                }
                // Admission does not choose a new semantic error for legacy
                // JSON. Reserve the representable coefficient bound and let
                // the existing numeric parser report the original error.
                let scale = usize::try_from(crate::types::extended::numeric::MAX_DSCALE)
                    .map_err(|_| CassieError::ResourceLimit("numeric workspace overflow".into()))?;
                Ok(add(
                    add(crate::types::extended::numeric::MAX_INTEGER_DIGITS, scale)?,
                    value.get().len(),
                )?)
            };
            // Exponents can make a short numeric token allocate a wide integer.
            // Inspect only visited scalar comparisons: unrelated legacy JSON
            // numbers retain their existing lazy containment behavior.
            self.reserve(mul(add(bound(candidate)?, bound(required)?)?, 8)?)?
        } else {
            None
        };
        super::scalar_equal(candidate, required)
    }
}

fn string_equal(
    candidate: &str,
    required: &str,
    controls: &QueryExecutionControls,
) -> Result<bool, QueryError> {
    let candidate = decode_string(candidate, controls)?;
    let required = decode_string(required, controls)?;
    if candidate.get().len() != required.get().len() {
        return Ok(false);
    }
    for (left, right) in candidate
        .get()
        .as_bytes()
        .chunks(1024)
        .zip(required.get().as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        let equal = left == right;
        #[cfg(test)]
        CANCEL_AFTER_STRING_COMPARE.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.cancel();
            }
        });
        controls.check_interruption()?;
        if !equal {
            return Ok(false);
        }
    }
    Ok(true)
}

fn decode_string(
    source: &str,
    controls: &QueryExecutionControls,
) -> Result<crate::runtime::accounted::Accounted<String>, QueryError> {
    use crate::runtime::accounted::json_arena::raw_decode;
    #[cfg(not(test))]
    let decoded = raw_decode::decode(source, controls);
    #[cfg(test)]
    let decoded = raw_decode::decode_observing(source, controls, || {
        CANCEL_AFTER_STRING_COPY.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.cancel();
            }
        });
    });
    decoded.map_err(|error| match error.into_control() {
        Ok(control) => control.into(),
        Err(error) => QueryError::General(error.to_string()),
    })
}

enum State {
    Objects {
        candidate: BTreeMap<String, Box<RawValue>>,
        required: btree_map::IntoIter<String, Box<RawValue>>,
    },
    Arrays {
        candidate: Vec<Box<RawValue>>,
        required: std::vec::IntoIter<Box<RawValue>>,
        current: Option<Box<RawValue>>,
        next_candidate: usize,
    },
}

struct Frame {
    state: State,
    parent: Option<Box<Self>>,
    _memory: Option<QueryMemoryReservation>,
}

enum Next {
    Child(Pair),
    Complete(bool),
}

impl Frame {
    fn advance(&mut self, previous: Option<bool>, work: &Work<'_>) -> Result<Next, QueryError> {
        work.check()?;
        match &mut self.state {
            State::Objects {
                candidate,
                required,
            } => {
                if previous == Some(false) {
                    return Ok(Next::Complete(false));
                }
                let Some((key, required)) = required.next() else {
                    return Ok(Next::Complete(true));
                };
                let Some(candidate) = candidate.get(&key) else {
                    return Ok(Next::Complete(false));
                };
                work.pair(candidate.get(), required.get()).map(Next::Child)
            }
            State::Arrays {
                candidate,
                required,
                current,
                next_candidate,
            } => {
                if previous == Some(true) {
                    *current = None;
                }
                if current.is_none() {
                    *current = required.next();
                    *next_candidate = 0;
                }
                let Some(required) = current else {
                    return Ok(Next::Complete(true));
                };
                let Some(candidate) = candidate.get(*next_candidate) else {
                    return Ok(Next::Complete(false));
                };
                *next_candidate += 1;
                work.pair(candidate.get(), required.get()).map(Next::Child)
            }
        }
    }
}

#[derive(Default)]
struct Frames(Option<Box<Frame>>);

impl Frames {
    fn push(&mut self, mut frame: Box<Frame>) {
        frame.parent = self.0.take();
        self.0 = Some(frame);
    }

    fn pop(&mut self) -> Option<Box<Frame>> {
        let mut frame = self.0.take()?;
        self.0 = frame.parent.take();
        Some(frame)
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        // Error/cancellation cleanup must not recursively drop a linked stack.
        while self.pop().is_some() {}
    }
}

enum Comparison {
    Container(Box<Frame>),
    Result { matched: bool, scalar: bool },
}

fn compare(pair: Pair, root: bool, work: &Work<'_>) -> Result<Comparison, QueryError> {
    work.check()?;
    let Pair {
        candidate,
        required,
        memory,
    } = pair;
    let state = match (kind(&candidate), kind(&required)) {
        (JsonKind::Object, JsonKind::Object) => State::Objects {
            candidate: json_document::parse_object(candidate.get())?.0,
            required: json_document::parse_object(required.get())?.0.into_iter(),
        },
        (JsonKind::Array, required_kind)
            if required_kind == JsonKind::Array || (root && required_kind.is_scalar()) =>
        {
            let required = if required_kind == JsonKind::Array {
                json_document::parse_array(required.get())?
            } else {
                vec![required]
            };
            State::Arrays {
                candidate: json_document::parse_array(candidate.get())?,
                required: required.into_iter(),
                current: None,
                next_candidate: 0,
            }
        }
        (JsonKind::Number, JsonKind::Number)
        | (JsonKind::String, JsonKind::String)
        | (JsonKind::Boolean, JsonKind::Boolean)
        | (JsonKind::Null, JsonKind::Null) => {
            return work
                .scalar_equal(&candidate, &required)
                .map(|matched| Comparison::Result {
                    matched,
                    scalar: true,
                });
        }
        _ => {
            return Ok(Comparison::Result {
                matched: false,
                scalar: false,
            })
        }
    };
    Ok(Comparison::Container(Box::new(Frame {
        state,
        parent: None,
        _memory: memory,
    })))
}

pub(super) fn evaluate(
    candidate: &str,
    required: &str,
    controls: Option<&QueryExecutionControls>,
    mut after_scalar: impl FnMut(bool),
) -> Result<bool, QueryError> {
    if let Some(controls) = controls {
        if let Some(result) = super::super::json_delete::controlled_containment_objects(
            candidate,
            required,
            controls,
            &mut after_scalar,
        )? {
            return Ok(result);
        }
    }
    evaluate_from(candidate, required, controls, true, after_scalar)
}

pub(super) fn evaluate_nested(
    candidate: &str,
    required: &str,
    controls: &QueryExecutionControls,
    after_scalar: &mut impl FnMut(bool),
) -> Result<bool, QueryError> {
    evaluate_from(candidate, required, Some(controls), false, after_scalar)
}

fn evaluate_from(
    candidate: &str,
    required: &str,
    controls: Option<&QueryExecutionControls>,
    root: bool,
    mut after_scalar: impl FnMut(bool),
) -> Result<bool, QueryError> {
    let work = Work { controls };
    let mut frames = Frames::default();
    let mut comparison = compare(work.pair(candidate, required)?, root, &work)?;
    loop {
        work.check()?;
        let (mut frame, previous) = match comparison {
            Comparison::Container(frame) => (frame, None),
            Comparison::Result {
                matched: result,
                scalar,
            } => {
                if scalar {
                    after_scalar(result);
                    work.check()?;
                }
                let Some(frame) = frames.pop() else {
                    return Ok(result);
                };
                (frame, Some(result))
            }
        };
        match frame.advance(previous, &work)? {
            Next::Child(pair) => {
                frames.push(frame);
                comparison = compare(pair, false, &work)?;
            }
            Next::Complete(result) => {
                comparison = Comparison::Result {
                    matched: result,
                    scalar: false,
                }
            }
        }
    }
}
