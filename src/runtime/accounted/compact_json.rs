//! Interruptible compact serialization of an already-owned serde JSON value.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "compact_json_tests.rs"]
mod tests;

pub(crate) struct OwnedJsonText {
    text: String,
    memory: QueryMemoryReservation,
}

impl OwnedJsonText {
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn into_parts(self) -> (String, QueryMemoryReservation) {
        (self.text, self.memory)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Progress {
    Scan,
    Copy,
    FrameMove,
}

enum Frame<'a> {
    Value(&'a serde_json::Value),
    Array(std::slice::Iter<'a, serde_json::Value>, bool),
    Object(serde_json::map::Iter<'a>, bool),
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
        progress: &mut impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.frames.len() == self.frames.capacity() {
            let capacity = next_capacity(self.frames.len())?;
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
                progress(Progress::FrameMove);
                self.controls.check_interruption()?;
            }
            let len = replacement.frames.len();
            for index in 0..len / 2 {
                self.controls.check_interruption()?;
                replacement.frames.swap(index, len - index - 1);
            }
            let old = std::mem::replace(&mut self.frames, replacement.frames);
            let old_memory = std::mem::replace(&mut self.memory, replacement.memory);
            drop(old);
            drop(old_memory);
        }
        self.frames.push(frame);
        Ok(())
    }
}

pub(super) struct Sink<'a> {
    text: String,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
    count: Option<usize>,
}

impl<'a> Sink<'a> {
    pub(super) fn for_arena(controls: &'a QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            text: String::new(),
            memory: controls.reserve_query_memory(0)?,
            controls,
            count: None,
        })
    }

    pub(super) fn finish_output(self) -> OwnedJsonText {
        OwnedJsonText {
            text: self.text,
            memory: self.memory,
        }
    }

    pub(super) fn write(
        &mut self,
        text: &str,
        progress: &mut impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if let Some(count) = &mut self.count {
            *count = count.checked_add(text.len()).ok_or_else(overflow)?;
            return Ok(());
        }
        let needed = self
            .text
            .len()
            .checked_add(text.len())
            .ok_or_else(overflow)?;
        if needed > self.text.capacity() {
            let capacity = next_capacity(needed.saturating_sub(1))?.max(8);
            let memory = self.controls.reserve_query_memory(capacity)?;
            let mut replacement = String::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.capacity() > capacity {
                return Err(overflow());
            }
            copy_chunks(&mut replacement, &self.text, self.controls, progress)?;
            let old = std::mem::replace(&mut self.text, replacement);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old);
            drop(old_memory);
        }
        copy_chunks(&mut self.text, text, self.controls, progress)
    }

    pub(super) fn quoted(
        &mut self,
        text: &str,
        progress: &mut impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.write("\"", progress)?;
        let mut start = 0;
        for (offset, character) in text.char_indices() {
            if offset - start >= 1024 {
                progress(Progress::Scan);
                self.controls.check_interruption()?;
                self.write(&text[start..offset], progress)?;
                start = offset;
            }
            let escape = match character {
                '"' => Some("\\\""),
                '\\' => Some("\\\\"),
                '\u{8}' => Some("\\b"),
                '\u{c}' => Some("\\f"),
                '\n' => Some("\\n"),
                '\r' => Some("\\r"),
                '\t' => Some("\\t"),
                _ => None,
            };
            if escape.is_some() || character <= '\u{1f}' {
                self.write(&text[start..offset], progress)?;
                if let Some(escape) = escape {
                    self.write(escape, progress)?;
                } else {
                    let byte = u8::try_from(u32::from(character)).map_err(|_| overflow())?;
                    let hex = b"0123456789abcdef";
                    let encoded = [
                        b'\\',
                        b'u',
                        b'0',
                        b'0',
                        hex[usize::from(byte >> 4)],
                        hex[usize::from(byte & 15)],
                    ];
                    let encoded = std::str::from_utf8(&encoded).map_err(|_| overflow())?;
                    self.write(encoded, progress)?;
                }
                start = offset + character.len_utf8();
            }
        }
        self.write(&text[start..], progress)?;
        self.write("\"", progress)
    }
}

fn copy_chunks(
    target: &mut String,
    text: &str,
    controls: &QueryExecutionControls,
    progress: &mut impl FnMut(Progress),
) -> Result<(), CassieError> {
    let mut start = 0;
    while start < text.len() {
        controls.check_interruption()?;
        let mut end = start.saturating_add(1024).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        target.push_str(&text[start..end]);
        progress(Progress::Copy);
        controls.check_interruption()?;
        start = end;
    }
    Ok(())
}

fn next_capacity(current: usize) -> Result<usize, CassieError> {
    current
        .checked_add(1)
        .and_then(usize::checked_next_power_of_two)
        .ok_or_else(overflow)
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("compact JSON output exceeded its admitted capacity".into())
}

pub(crate) fn serialize(
    value: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<OwnedJsonText, CassieError> {
    serialize_observing(value, controls, |_| {})
}

fn serialize_observing(
    value: &serde_json::Value,
    controls: &QueryExecutionControls,
    progress: impl FnMut(Progress),
) -> Result<OwnedJsonText, CassieError> {
    let sink = render(value, controls, false, progress)?;
    Ok(OwnedJsonText {
        text: sink.text,
        memory: sink.memory,
    })
}

pub(crate) fn count(
    value: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    count_observing(value, controls, |_| {})
}

fn count_observing(
    value: &serde_json::Value,
    controls: &QueryExecutionControls,
    progress: impl FnMut(Progress),
) -> Result<usize, CassieError> {
    let sink = render(value, controls, true, progress)?;
    Ok(sink.count.expect("counting sink"))
}

fn render<'a>(
    value: &'a serde_json::Value,
    controls: &'a QueryExecutionControls,
    counting: bool,
    mut progress: impl FnMut(Progress),
) -> Result<Sink<'a>, CassieError> {
    controls.check_interruption()?;
    let mut sink = Sink {
        text: String::new(),
        memory: controls.reserve_query_memory(0)?,
        controls,
        count: counting.then_some(0),
    };
    let mut stack = Stack::new(controls)?;
    stack.push(Frame::Value(value), &mut progress)?;
    while let Some(frame) = stack.frames.pop() {
        controls.check_interruption()?;
        match frame {
            Frame::Value(value) => match value {
                serde_json::Value::Null => sink.write("null", &mut progress)?,
                serde_json::Value::Bool(value) => {
                    sink.write(if *value { "true" } else { "false" }, &mut progress)?
                }
                serde_json::Value::Number(value) => sink.write(value.as_str(), &mut progress)?,
                serde_json::Value::String(value) => sink.quoted(value, &mut progress)?,
                serde_json::Value::Array(values) => {
                    sink.write("[", &mut progress)?;
                    stack.push(Frame::Array(values.iter(), false), &mut progress)?;
                }
                serde_json::Value::Object(values) => {
                    sink.write("{", &mut progress)?;
                    stack.push(Frame::Object(values.iter(), false), &mut progress)?;
                }
            },
            Frame::Array(mut values, emitted) => {
                if let Some(value) = values.next() {
                    if emitted {
                        sink.write(",", &mut progress)?;
                    }
                    stack.push(Frame::Array(values, true), &mut progress)?;
                    stack.push(Frame::Value(value), &mut progress)?;
                } else {
                    sink.write("]", &mut progress)?;
                }
            }
            Frame::Object(mut values, emitted) => {
                if let Some((key, value)) = values.next() {
                    if emitted {
                        sink.write(",", &mut progress)?;
                    }
                    sink.quoted(key, &mut progress)?;
                    sink.write(":", &mut progress)?;
                    stack.push(Frame::Object(values, true), &mut progress)?;
                    stack.push(Frame::Value(value), &mut progress)?;
                } else {
                    sink.write("}", &mut progress)?;
                }
            }
        }
    }
    Ok(sink)
}
