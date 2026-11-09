//! Owned output for a raw object deletion, preserving lazy child decoding.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress as SinkProgress, Sink};
use crate::runtime::accounted::json_arena::{raw_decode, raw_number};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::object::{self, ObjectError, OrderedFields};
use super::{scan, Progress as ScanProgress, ScanError, Scanner};

#[cfg(test)]
#[path = "json_delete_raw_render_tests.rs"]
mod tests;

#[path = "json_delete_raw_path.rs"]
pub(in crate::executor::filter) mod path;
#[cfg(test)]
use path::render_path;

#[derive(Debug)]
pub(in crate::executor::filter) enum RenderError {
    Control(CassieError),
    Scan(ScanError),
    Object(ObjectError),
    Decode(raw_decode::DecodeError),
    InvalidValue,
    Numeric,
    Path(crate::executor::QueryError),
}

impl RenderError {
    pub(in crate::executor::filter) fn into_query_error(self) -> crate::executor::QueryError {
        match self {
            Self::Control(error) => error.into(),
            Self::Scan(error) => {
                crate::executor::QueryError::General(format!("invalid JSON document: {error}"))
            }
            Self::Object(error) => {
                crate::executor::QueryError::General(format!("invalid JSON document: {error}"))
            }
            Self::Decode(error) => {
                crate::executor::QueryError::General(format!("invalid JSON document: {error}"))
            }
            Self::Numeric => {
                crate::executor::QueryError::General("value overflows numeric format".into())
            }
            Self::InvalidValue => crate::executor::QueryError::General("invalid JSON value".into()),
            Self::Path(error) => error,
        }
    }
}

impl From<CassieError> for RenderError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

impl From<ScanError> for RenderError {
    fn from(error: ScanError) -> Self {
        match error {
            ScanError::Control(error) => Self::Control(error),
            other => Self::Scan(other),
        }
    }
}

impl From<ObjectError> for RenderError {
    fn from(error: ObjectError) -> Self {
        match error {
            ObjectError::Control(error) => Self::Control(error),
            other => Self::Object(other),
        }
    }
}

#[derive(Clone, Copy)]
enum Progress {
    OutputCopy,
    FrameMove,
}

#[derive(Clone, Copy)]
enum ArraySkip<'a> {
    Index(usize),
    Keys(KeySkip<'a>),
}

#[derive(Clone, Copy)]
enum KeySkip<'a> {
    One(&'a str),
    Many(&'a [Option<String>]),
    Native(&'a [crate::types::Value]),
    Arena(&'a crate::runtime::accounted::json_arena::OwnedTextArray),
}

enum Frame<'a> {
    Value(&'a str),
    Object {
        fields: OrderedFields<'a>,
        next: usize,
        emitted: bool,
        skip: Option<KeySkip<'a>>,
        replace: Option<(&'a str, &'a str)>,
    },
    Array {
        source: &'a str,
        scanner: Scanner<'a, fn(ScanProgress)>,
        next: usize,
        emitted: bool,
        skip: Option<ArraySkip<'a>>,
        replace: Option<(usize, &'a str)>,
    },
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
    fn push(
        &mut self,
        frame: Frame<'a>,
        report: &mut impl FnMut(SinkProgress),
    ) -> Result<(), CassieError> {
        if self.frames.len() == self.frames.capacity() {
            let capacity = self
                .frames
                .len()
                .checked_mul(2)
                .unwrap_or(usize::MAX)
                .max(4);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<Frame<'_>>())
                .ok_or_else(|| CassieError::ResourceLimit("JSON output frame overflow".into()))?;
            let mut replacement = Replacement {
                frames: Vec::new(),
                memory: self.controls.reserve_query_memory(bytes)?,
            };
            replacement
                .frames
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.frames.capacity() > capacity {
                return Err(CassieError::ResourceLimit(
                    "JSON output frame exceeded its admission".into(),
                ));
            }
            while let Some(old) = self.frames.pop() {
                self.controls.check_interruption()?;
                replacement.frames.push(old);
                report(SinkProgress::FrameMove);
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

pub(in crate::executor::filter) fn render_object_without_key<'a>(
    source: &'a str,
    key: &'a str,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    render_observing(source, key, controls, |_| {})
}

fn render_observing<'a>(
    source: &'a str,
    key: &'a str,
    controls: &'a QueryExecutionControls,
    observe: impl FnMut(Progress),
) -> Result<OwnedJsonText, RenderError> {
    let fields = object::parse(source, controls)?.into_ordered(controls)?;
    render_frames(
        Frame::Object {
            fields,
            next: 0,
            emitted: false,
            skip: Some(KeySkip::One(key)),
            replace: None,
        },
        controls,
        observe,
    )
}

pub(in crate::executor::filter) fn render_array_without_index<'a>(
    source: &'a str,
    index: i64,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    let count = array_count(source, controls)?;
    let skip = super::super::normalize_array_index(index, count).map(ArraySkip::Index);
    let mut scanner = array_scanner(source, controls)?;
    scanner.next()?;
    render_frames(
        Frame::Array {
            source,
            scanner,
            next: 0,
            emitted: false,
            skip,
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(in crate::executor::filter) fn render_array_without_text<'a>(
    source: &'a str,
    key: &'a str,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    array_count(source, controls)?;
    let mut scanner = array_scanner(source, controls)?;
    scanner.next()?;
    render_frames(
        Frame::Array {
            source,
            scanner,
            next: 0,
            emitted: false,
            skip: Some(ArraySkip::Keys(KeySkip::One(key))),
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(in crate::executor::filter) fn render_object_without_keys<'a>(
    source: &'a str,
    keys: &'a [Option<String>],
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    let fields = object::parse(source, controls)?.into_ordered(controls)?;
    render_frames(
        Frame::Object {
            fields,
            next: 0,
            emitted: false,
            skip: Some(KeySkip::Many(keys)),
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(in crate::executor::filter) fn render_array_without_keys<'a>(
    source: &'a str,
    keys: &'a [Option<String>],
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    array_count(source, controls)?;
    let mut has_key = false;
    for key in keys {
        controls.check_interruption()?;
        if key.is_some() {
            has_key = true;
            break;
        }
    }
    let mut scanner = array_scanner(source, controls)?;
    scanner.next()?;
    render_frames(
        Frame::Array {
            source,
            scanner,
            next: 0,
            emitted: false,
            skip: has_key.then_some(ArraySkip::Keys(KeySkip::Many(keys))),
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(in crate::executor::filter) fn native_keys_valid(
    keys: &[crate::types::Value],
    controls: &QueryExecutionControls,
) -> Result<bool, RenderError> {
    let mut has_key = false;
    for key in keys {
        controls.check_interruption()?;
        match key {
            crate::types::Value::String(_) => has_key = true,
            crate::types::Value::Null => {}
            _ => {
                return Err(RenderError::Path(
                    super::super::unsupported_subtraction_operand(),
                ))
            }
        }
    }
    Ok(has_key)
}

pub(in crate::executor::filter) fn render_object_without_native_keys<'a>(
    source: &'a str,
    keys: &'a [crate::types::Value],
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    native_keys_valid(keys, controls)?;
    let fields = object::parse(source, controls)?.into_ordered(controls)?;
    render_frames(
        Frame::Object {
            fields,
            next: 0,
            emitted: false,
            skip: Some(KeySkip::Native(keys)),
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(in crate::executor::filter) fn render_array_without_native_keys<'a>(
    source: &'a str,
    keys: &'a [crate::types::Value],
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    let has_key = native_keys_valid(keys, controls)?;
    array_count(source, controls)?;
    let mut scanner = array_scanner(source, controls)?;
    scanner.next()?;
    render_frames(
        Frame::Array {
            source,
            scanner,
            next: 0,
            emitted: false,
            skip: has_key.then_some(ArraySkip::Keys(KeySkip::Native(keys))),
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(in crate::executor::filter) fn render_object_without_arena_keys<'a>(
    source: &'a str,
    keys: &'a crate::runtime::accounted::json_arena::OwnedTextArray,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    let fields = object::parse(source, controls)?.into_ordered(controls)?;
    render_frames(
        Frame::Object {
            fields,
            next: 0,
            emitted: false,
            skip: Some(KeySkip::Arena(keys)),
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(in crate::executor::filter) fn render_array_without_arena_keys<'a>(
    source: &'a str,
    keys: &'a crate::runtime::accounted::json_arena::OwnedTextArray,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    array_count(source, controls)?;
    let mut has_key = false;
    for index in 0..keys.len() {
        controls.check_interruption()?;
        if keys.get(index)?.is_some() {
            has_key = true;
            break;
        }
    }
    let mut scanner = array_scanner(source, controls)?;
    scanner.next()?;
    render_frames(
        Frame::Array {
            source,
            scanner,
            next: 0,
            emitted: false,
            skip: has_key.then_some(ArraySkip::Keys(KeySkip::Arena(keys))),
            replace: None,
        },
        controls,
        |_| {},
    )
}

pub(super) fn render_value<'a>(
    source: &'a str,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    render_frames(Frame::Value(source), controls, |_| {})
}

fn render_frames<'a>(
    first: Frame<'a>,
    controls: &'a QueryExecutionControls,
    mut observe: impl FnMut(Progress),
) -> Result<OwnedJsonText, RenderError> {
    let mut sink = Sink::for_arena(controls)?;
    let mut stack = Stack::new(controls)?;
    let mut report = |progress| match progress {
        SinkProgress::Copy => observe(Progress::OutputCopy),
        SinkProgress::FrameMove => observe(Progress::FrameMove),
        SinkProgress::Scan => {}
    };
    match &first {
        Frame::Object { .. } => sink.write("{", &mut report)?,
        Frame::Array { .. } => sink.write("[", &mut report)?,
        Frame::Value(_) => {}
    }
    stack.push(first, &mut report)?;
    while let Some(frame) = stack.frames.pop() {
        controls.check_interruption()?;
        match frame {
            Frame::Value(source) => {
                // Every value frame comes from an exact scanner span.
                let token = source;
                match token.as_bytes().first().copied() {
                    Some(b'{') => {
                        let fields = object::parse(token, controls)?.into_ordered(controls)?;
                        sink.write("{", &mut report)?;
                        stack.push(
                            Frame::Object {
                                fields,
                                next: 0,
                                emitted: false,
                                skip: None,
                                replace: None,
                            },
                            &mut report,
                        )?;
                    }
                    Some(b'[') => {
                        scan(token, controls)?;
                        let mut scanner = Scanner {
                            input: token.as_bytes(),
                            position: 0,
                            line: 1,
                            column: 0,
                            controls,
                            observe: ignore_scan as fn(ScanProgress),
                        };
                        scanner.next()?;
                        sink.write("[", &mut report)?;
                        stack.push(
                            Frame::Array {
                                source: token,
                                scanner,
                                next: 0,
                                emitted: false,
                                skip: None,
                                replace: None,
                            },
                            &mut report,
                        )?;
                    }
                    Some(b'"') => {
                        let text =
                            raw_decode::decode(token, controls).map_err(|error| {
                                match error.into_control() {
                                    Ok(error) => RenderError::Control(error),
                                    Err(error) => RenderError::Decode(error),
                                }
                            })?;
                        sink.quoted(text.get(), &mut report)?;
                    }
                    Some(b'-' | b'0'..=b'9') => {
                        let text =
                            raw_number::format(token, controls).map_err(|error| match error {
                                raw_number::RawNumberError::Overflow => RenderError::Numeric,
                                raw_number::RawNumberError::Control(error) => {
                                    RenderError::Control(error)
                                }
                            })?;
                        sink.write(text.get(), &mut report)?;
                    }
                    Some(b'n' | b't' | b'f') => sink.write(token, &mut report)?,
                    _ => return Err(RenderError::InvalidValue),
                }
            }
            Frame::Object {
                fields,
                mut next,
                mut emitted,
                skip,
                replace,
            } => {
                let mut selected = None;
                while let Some((name, value)) = fields.get(next) {
                    controls.check_interruption()?;
                    next += 1;
                    if let Some(skip) = skip {
                        if matches_key(name, skip, controls)? {
                            continue;
                        }
                    }
                    if emitted {
                        sink.write(", ", &mut report)?;
                    }
                    sink.quoted(name, &mut report)?;
                    sink.write(": ", &mut report)?;
                    emitted = true;
                    selected = Some(if let Some((target, replacement)) = replace {
                        if object::equal(name, target, controls)? {
                            replacement
                        } else {
                            value
                        }
                    } else {
                        value
                    });
                    break;
                }
                if let Some(value) = selected {
                    stack.push(
                        Frame::Object {
                            fields,
                            next,
                            emitted,
                            skip,
                            replace,
                        },
                        &mut report,
                    )?;
                    stack.push(Frame::Value(value), &mut report)?;
                } else {
                    sink.write("}", &mut report)?;
                }
            }
            Frame::Array {
                source,
                mut scanner,
                next,
                mut emitted,
                skip,
                replace,
            } => {
                scanner.whitespace()?;
                if scanner.peek() == Some(b']') {
                    scanner.next()?;
                    sink.write("]", &mut report)?;
                    continue;
                }
                if next > 0 {
                    scanner.next()?;
                    scanner.whitespace()?;
                }
                let span = scanner.value_span()?;
                let child = &source[span];
                let following = next.checked_add(1).ok_or_else(|| {
                    CassieError::ResourceLimit("JSON array index overflow".into())
                })?;
                let removed = match skip {
                    Some(ArraySkip::Index(index)) => index == next,
                    Some(ArraySkip::Keys(keys)) if child.starts_with('"') => {
                        match raw_decode::decode(child, controls) {
                            Ok(decoded) => matches_key(decoded.get(), keys, controls)?,
                            Err(error) => match error.into_control() {
                                Ok(error) => return Err(error.into()),
                                Err(_) => false,
                            },
                        }
                    }
                    Some(ArraySkip::Keys(_)) | None => false,
                };
                if !removed && emitted {
                    sink.write(", ", &mut report)?;
                }
                if !removed {
                    emitted = true;
                }
                stack.push(
                    Frame::Array {
                        source,
                        scanner,
                        next: following,
                        emitted,
                        skip,
                        replace,
                    },
                    &mut report,
                )?;
                if !removed {
                    let child = match replace {
                        Some((index, replacement)) if index == next => replacement,
                        _ => child,
                    };
                    stack.push(Frame::Value(child), &mut report)?;
                }
            }
        }
    }
    Ok(sink.finish_output())
}

fn ignore_scan(_: ScanProgress) {}

fn matches_key(
    value: &str,
    keys: KeySkip<'_>,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    match keys {
        KeySkip::One(key) => object::equal(value, key, controls),
        KeySkip::Many(keys) => {
            for key in keys {
                controls.check_interruption()?;
                if let Some(key) = key {
                    if object::equal(value, key, controls)? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        KeySkip::Native(keys) => {
            for key in keys {
                controls.check_interruption()?;
                if let crate::types::Value::String(key) = key {
                    if object::equal(value, key, controls)? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        KeySkip::Arena(keys) => {
            for index in 0..keys.len() {
                controls.check_interruption()?;
                if let Some(key) = keys.get(index)? {
                    if object::equal(value, key, controls)? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
    }
}

pub(super) fn array_scanner<'a>(
    source: &'a str,
    controls: &'a QueryExecutionControls,
) -> Result<Scanner<'a, fn(ScanProgress)>, RenderError> {
    controls.check_interruption()?;
    let mut scanner = Scanner {
        input: source.as_bytes(),
        position: 0,
        line: 1,
        column: 0,
        controls,
        observe: ignore_scan as fn(ScanProgress),
    };
    scanner.whitespace()?;
    if scanner.peek() != Some(b'[') {
        return Err(RenderError::InvalidValue);
    }
    Ok(scanner)
}

pub(super) fn array_count(
    source: &str,
    controls: &QueryExecutionControls,
) -> Result<usize, RenderError> {
    let mut scanner = array_scanner(source, controls)?;
    scanner.next()?;
    let mut count = 0_usize;
    loop {
        scanner.whitespace()?;
        match scanner.peek() {
            Some(b']') => {
                scanner.next()?;
                break;
            }
            None => return Err(scanner.error("EOF while parsing a list").into()),
            Some(b',') if count > 0 => {
                scanner.next()?;
                scanner.whitespace()?;
                match scanner.peek() {
                    Some(b']') => return Err(scanner.error("trailing comma").into()),
                    None => return Err(scanner.error("EOF while parsing a value").into()),
                    Some(_) => {}
                }
            }
            Some(_) if count > 0 => return Err(scanner.error("expected `,` or `]`").into()),
            Some(_) => {}
        }
        scanner.value_span()?;
        count = count
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("JSON array index overflow".into()))?;
    }
    scanner.whitespace()?;
    if scanner.peek().is_some() {
        return Err(scanner.error("trailing characters").into());
    }
    Ok(count)
}
