//! Syntax-only JSON scan for deletion: raw strings remain undecoded.
use std::fmt;
use std::ops::Range;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "json_delete_raw_scan_tests.rs"]
mod tests;

#[path = "json_extract_raw.rs"]
pub(super) mod extract;
#[path = "json_delete_raw_object.rs"]
mod object;
#[path = "json_delete_raw_render.rs"]
pub(in crate::executor::filter) mod render;

#[derive(Debug)]
enum ScanError {
    Control(CassieError),
    Syntax {
        message: &'static str,
        line: usize,
        column: usize,
    },
}

impl From<CassieError> for ScanError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(error) => write!(out, "{error}"),
            Self::Syntax {
                message,
                line,
                column,
            } => {
                write!(out, "{message} at line {line} column {column}")
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Frame {
    ArrayFirst,
    ArrayValue,
    ArrayAfter,
    ObjectFirst,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectAfter,
}

#[derive(Clone, Copy)]
enum Progress {
    Input,
    StackMove,
}

struct Stack<'a> {
    frames: Vec<Frame>,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
}

struct Replacement {
    frames: Vec<Frame>,
    memory: QueryMemoryReservation,
}

impl<'a> Stack<'a> {
    fn new(controls: &'a QueryExecutionControls) -> Result<Self, ScanError> {
        Ok(Self {
            frames: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
            controls,
        })
    }

    fn push(&mut self, frame: Frame, observer: &mut impl FnMut(Progress)) -> Result<(), ScanError> {
        if self.frames.len() == self.frames.capacity() {
            let capacity = self
                .frames
                .len()
                .checked_mul(2)
                .unwrap_or(usize::MAX)
                .max(4);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<Frame>())
                .ok_or_else(|| CassieError::ResourceLimit("JSON raw scan stack overflow".into()))?;
            let memory = self.controls.reserve_query_memory(bytes)?;
            let mut replacement = Replacement {
                frames: Vec::new(),
                memory,
            };
            replacement
                .frames
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.frames.capacity() > capacity {
                return Err(CassieError::ResourceLimit(
                    "JSON raw scan stack exceeded its admission".into(),
                )
                .into());
            }
            while let Some(old) = self.frames.pop() {
                self.controls.check_interruption()?;
                replacement.frames.push(old);
                observer(Progress::StackMove);
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

struct Scanner<'a, F> {
    input: &'a [u8],
    position: usize,
    line: usize,
    column: usize,
    controls: &'a QueryExecutionControls,
    observe: F,
}

fn scan(input: &str, controls: &QueryExecutionControls) -> Result<Range<usize>, ScanError> {
    scan_observing(input, controls, |_| {})
}

#[cfg(test)]
fn scan_prefix(input: &str, controls: &QueryExecutionControls) -> Result<Range<usize>, ScanError> {
    scan_inner(input, controls, false, |_| {})
}

fn scan_observing(
    input: &str,
    controls: &QueryExecutionControls,
    observer: impl FnMut(Progress),
) -> Result<Range<usize>, ScanError> {
    scan_inner(input, controls, true, observer)
}

fn scan_inner(
    input: &str,
    controls: &QueryExecutionControls,
    complete: bool,
    observer: impl FnMut(Progress),
) -> Result<Range<usize>, ScanError> {
    controls.check_interruption()?;
    let mut scanner = Scanner {
        input: input.as_bytes(),
        position: 0,
        line: 1,
        column: 0,
        controls,
        observe: observer,
    };
    let range = scanner.value_span()?;
    if complete {
        scanner.whitespace()?;
        if scanner.peek().is_some() {
            return Err(scanner.error("trailing characters"));
        }
    }
    Ok(range)
}

impl<F: FnMut(Progress)> Scanner<'_, F> {
    fn value_span(&mut self) -> Result<Range<usize>, ScanError> {
        let mut stack = Stack::new(self.controls)?;
        self.whitespace()?;
        let start = self.position;
        self.value(&mut stack)?;
        while let Some(frame) = stack.frames.pop() {
            self.whitespace()?;
            match frame {
                Frame::ArrayFirst | Frame::ArrayValue => {
                    if self.peek() == Some(b']') {
                        if matches!(frame, Frame::ArrayValue) {
                            return Err(self.error("expected value"));
                        }
                        let _consumed = self.next()?;
                    } else {
                        if matches!(frame, Frame::ArrayFirst) && self.peek().is_none() {
                            return Err(self.error("EOF while parsing a list"));
                        }
                        stack.push(Frame::ArrayAfter, &mut self.observe)?;
                        self.value(&mut stack)?;
                    }
                }
                Frame::ArrayAfter => match self.peek() {
                    Some(b']') => {
                        let _consumed = self.next()?;
                    }
                    Some(b',') => {
                        let _consumed = self.next()?;
                        stack.push(Frame::ArrayValue, &mut self.observe)?;
                    }
                    Some(_) => return Err(self.error("expected `,` or `]`")),
                    None => return Err(self.error("EOF while parsing a list")),
                },
                Frame::ObjectFirst | Frame::ObjectKey => {
                    if self.peek() == Some(b'}') {
                        if matches!(frame, Frame::ObjectKey) {
                            return Err(self.error("key must be a string"));
                        }
                        let _consumed = self.next()?;
                    } else if self.peek() == Some(b'"') {
                        self.string()?;
                        stack.push(Frame::ObjectColon, &mut self.observe)?;
                    } else if self.peek().is_none() {
                        return Err(self.error("EOF while parsing an object"));
                    } else {
                        return Err(self.error("key must be a string"));
                    }
                }
                Frame::ObjectColon => match self.peek() {
                    Some(b':') => {
                        let _consumed = self.next()?;
                        stack.push(Frame::ObjectValue, &mut self.observe)?;
                    }
                    None => return Err(self.error("EOF while parsing an object")),
                    Some(_) => return Err(self.error("expected `:`")),
                },
                Frame::ObjectValue => {
                    stack.push(Frame::ObjectAfter, &mut self.observe)?;
                    self.value(&mut stack)?;
                }
                Frame::ObjectAfter => match self.peek() {
                    Some(b'}') => {
                        let _consumed = self.next()?;
                    }
                    Some(b',') => {
                        let _consumed = self.next()?;
                        stack.push(Frame::ObjectKey, &mut self.observe)?;
                    }
                    None => return Err(self.error("EOF while parsing an object")),
                    Some(_) => return Err(self.error("expected `,` or `}`")),
                },
            }
        }
        Ok(start..self.position)
    }
    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }
    fn next(&mut self) -> Result<Option<u8>, ScanError> {
        self.controls.check_interruption()?;
        let Some(byte) = self.peek() else {
            return Ok(None);
        };
        self.position += 1;
        if byte == b'\n' {
            self.line += 1;
            self.column = 0;
        } else {
            self.column += 1;
        }
        (self.observe)(Progress::Input);
        self.controls.check_interruption()?;
        Ok(Some(byte))
    }
    fn whitespace(&mut self) -> Result<(), ScanError> {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            let _consumed = self.next()?;
        }
        Ok(())
    }
    fn error(&self, message: &'static str) -> ScanError {
        if self.peek() == Some(b'\n') {
            return ScanError::Syntax {
                message,
                line: self.line + 1,
                column: 0,
            };
        }
        ScanError::Syntax {
            message,
            line: self.line,
            column: self.column + usize::from(self.peek().is_some()),
        }
    }
    fn value(&mut self, stack: &mut Stack<'_>) -> Result<(), ScanError> {
        match self.peek() {
            Some(b'[') => {
                let _consumed = self.next()?;
                stack.push(Frame::ArrayFirst, &mut self.observe)?;
            }
            Some(b'{') => {
                let _consumed = self.next()?;
                stack.push(Frame::ObjectFirst, &mut self.observe)?;
            }
            Some(b'"') => self.string()?,
            Some(b'-' | b'0'..=b'9') => self.number()?,
            Some(b'n') => self.literal(b"null")?,
            Some(b't') => self.literal(b"true")?,
            Some(b'f') => self.literal(b"false")?,
            Some(_) => return Err(self.error("expected value")),
            None => return Err(self.error("EOF while parsing a value")),
        }
        Ok(())
    }
    fn literal(&mut self, expected: &[u8]) -> Result<(), ScanError> {
        for byte in expected {
            match self.next()? {
                Some(actual) if actual == *byte => {}
                Some(_) => return Err(self.error_current("expected ident")),
                None => return Err(self.error("EOF while parsing a value")),
            }
        }
        Ok(())
    }
    fn error_current(&self, message: &'static str) -> ScanError {
        ScanError::Syntax {
            message,
            line: self.line,
            column: self.column,
        }
    }
    fn string(&mut self) -> Result<(), ScanError> {
        let _consumed = self.next()?;
        loop {
            if matches!(self.peek(), Some(0..=31)) {
                return Err(self.error_current(
                    "control character (\\u0000-\\u001F) found while parsing a string",
                ));
            }
            match self.next()? {
                None => return Err(self.error("EOF while parsing a string")),
                Some(b'"') => return Ok(()),
                Some(b'\\') => match self.next()? {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {}
                    Some(b'u') => {
                        let mut valid = true;
                        for _ in 0..4 {
                            match self.next()? {
                                Some(byte) => valid &= byte.is_ascii_hexdigit(),
                                None => return Err(self.error("EOF while parsing a string")),
                            }
                        }
                        if !valid {
                            return Err(self.error_current("invalid escape"));
                        }
                    }
                    Some(_) => return Err(self.error_current("invalid escape")),
                    None => return Err(self.error("EOF while parsing a string")),
                },
                Some(_) => {}
            }
        }
    }
    fn number(&mut self) -> Result<(), ScanError> {
        if self.peek() == Some(b'-') {
            let _consumed = self.next()?;
        }
        match self.next()? {
            Some(b'0') => {
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.error("invalid number"));
                }
            }
            Some(b'1'..=b'9') => self.digits()?,
            _ => return Err(self.error_current("invalid number")),
        }
        if self.peek() == Some(b'.') {
            let _consumed = self.next()?;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("invalid number"));
            }
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let _consumed = self.next()?;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                let _consumed = self.next()?;
            }
            if !matches!(self.next()?, Some(b'0'..=b'9')) {
                return Err(self.error_current("invalid number"));
            }
            self.digits()?;
        }
        Ok(())
    }
    fn digits(&mut self) -> Result<(), ScanError> {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            let _consumed = self.next()?;
        }
        Ok(())
    }
}
