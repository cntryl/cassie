//! One-pass, admitted sort tie text with iterative JSON/SQL-array traversal.
use std::fmt::{self, Write as _};

use crate::app::CassieError;
use crate::executor::batch::RowAccess;
use crate::executor::retained_memory::{add, mul};
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{SqlArray, Value};

use crate::executor::numeric_controlled_text as numeric;
#[cfg(test)]
#[path = "tie_tests.rs"]
mod tests;

pub(crate) struct TieText {
    text: String,
    memory: QueryMemoryReservation,
}
impl TieText {
    #[cfg(test)]
    fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn into_parts(self) -> (String, QueryMemoryReservation) {
        (self.text, self.memory)
    }
}

struct Sink<'a, F> {
    text: String,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
    progress: F,
    since_check: usize,
    before_first_growth: Option<&'a dyn Fn() -> Result<(), QueryError>>,
}
impl<F: FnMut(usize)> Sink<'_, F> {
    fn write(&mut self, text: &str) -> Result<(), QueryError> {
        self.write_observing_growth(text, |_| {})
    }

    fn write_observing_growth(
        &mut self,
        text: &str,
        mut after_copy: impl FnMut(usize),
    ) -> Result<(), QueryError> {
        let needed = add(self.text.len(), text.len())?;
        if needed > self.text.capacity() {
            self.controls.check_interruption()?;
            let capacity = needed
                .max(8)
                .checked_next_power_of_two()
                .ok_or_else(|| CassieError::ResourceLimit("sort tie capacity overflow".into()))?;
            let replacement_memory = self.controls.reserve_query_memory(capacity)?;
            if let Some(probe) = self.before_first_growth.take() {
                probe()?;
            }
            let mut replacement = String::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.capacity() > capacity {
                return Err(CassieError::ResourceLimit(
                    "allocator exceeded admitted sort tie capacity".into(),
                )
                .into());
            }
            let mut offset = 0;
            while offset < self.text.len() {
                let mut end = offset.saturating_add(1024).min(self.text.len());
                while !self.text.is_char_boundary(end) {
                    end -= 1;
                }
                replacement.push_str(&self.text[offset..end]);
                offset = end;
                after_copy(offset);
                self.controls.check_interruption()?;
            }
            self.text = replacement;
            self.memory = replacement_memory;
        }
        let mut offset = 0;
        while offset < text.len() {
            let mut end = offset.saturating_add(1024).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            self.text.push_str(&text[offset..end]);
            self.since_check = self.since_check.saturating_add(end - offset);
            offset = end;
            if self.since_check >= 1024 {
                (self.progress)(self.text.len());
                self.controls.check_interruption()?;
                self.since_check = 0;
            }
        }
        Ok(())
    }

    fn quoted(&mut self, text: &str) -> Result<(), QueryError> {
        self.write("\"")?;
        let mut start = 0;
        for (offset, character) in text.char_indices() {
            if offset - start >= 1024 {
                self.write(&text[start..offset])?;
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
                self.write(&text[start..offset])?;
                if let Some(escape) = escape {
                    self.write(escape)?;
                } else {
                    let byte = u8::try_from(u32::from(character)).map_err(|_| {
                        CassieError::Execution("invalid JSON control character".into())
                    })?;
                    let hex = b"0123456789abcdef";
                    let encoded = [
                        b'\\',
                        b'u',
                        b'0',
                        b'0',
                        hex[usize::from(byte >> 4)],
                        hex[usize::from(byte & 15)],
                    ];
                    self.write(
                        std::str::from_utf8(&encoded)
                            .map_err(|error| QueryError::General(error.to_string()))?,
                    )?;
                }
                start = offset + character.len_utf8();
            }
        }
        self.write(&text[start..])?;
        self.write("\"")
    }
}

struct Fixed {
    bytes: [u8; 1024],
    len: usize,
}
impl Fixed {
    fn new() -> Self {
        Self {
            bytes: [0; 1024],
            len: 0,
        }
    }
    fn as_str(&self) -> Result<&str, QueryError> {
        std::str::from_utf8(&self.bytes[..self.len])
            .map_err(|error| QueryError::General(error.to_string()))
    }
}
impl fmt::Write for Fixed {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let dest = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        dest.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
fn fixed(value: impl fmt::Display) -> Result<Fixed, QueryError> {
    let mut out = Fixed::new();
    write!(&mut out, "{value}")
        .map_err(|_| CassieError::ResourceLimit("sort tie scalar width exceeded".into()))?;
    Ok(out)
}

enum Frame<'a> {
    Json(&'a serde_json::Value),
    JsonArray(std::slice::Iter<'a, serde_json::Value>, bool),
    JsonObject(serde_json::map::Iter<'a>, bool),
    SqlValue(&'a Value),
    SqlArray {
        array: &'a SqlArray,
        depth: usize,
        next: usize,
        base: usize,
        stride: usize,
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
                .ok_or_else(|| CassieError::ResourceLimit("sort tie frame overflow".into()))?;
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
                return Err(CassieError::ResourceLimit(
                    "allocator exceeded admitted sort tie frame capacity".into(),
                )
                .into());
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
            let old_frames = std::mem::replace(&mut self.frames, replacement.frames);
            let old_memory = std::mem::replace(&mut self.memory, replacement.memory);
            drop(old_frames);
            drop(old_memory);
        }
        self.frames.push(frame);
        Ok(())
    }
}

fn typed<F: FnMut(usize)>(
    value: &crate::types::typed::TypedValue,
    quoted: bool,
    sink: &mut Sink<'_, F>,
) -> Result<(), QueryError> {
    use crate::types::typed::TypedValue;
    match value {
        TypedValue::Numeric(value) => {
            let text = numeric::format(value, sink.controls)?;
            if quoted {
                sink.quoted(text.text())
            } else {
                sink.write(text.text())
            }
        }
        TypedValue::Bit(bits) | TypedValue::VarBit(bits) => {
            if quoted {
                sink.write("\"")?;
            }
            for bit in bits.bits() {
                sink.write(if bit { "1" } else { "0" })?;
            }
            if quoted {
                sink.write("\"")?;
            }
            Ok(())
        }
        _ => {
            // All other typed carriers have fixed-width numeric/date/network
            // components; reserve their formatting scratch before conversion.
            let _scratch = sink.controls.reserve_query_memory(2048)?;
            let text = value.text();
            if text.capacity() > 2048 {
                return Err(CassieError::ResourceLimit(
                    "typed sort tie exceeded its admitted shape".into(),
                )
                .into());
            }
            if quoted {
                sink.quoted(&text)
            } else {
                sink.write(&text)
            }
        }
    }
}

fn float_json<F: FnMut(usize)>(value: f64, sink: &mut Sink<'_, F>) -> Result<(), QueryError> {
    let _scratch = sink.controls.reserve_query_memory(1024)?;
    match serde_json::Number::from_f64(value) {
        Some(number) => sink.write(fixed(number)?.as_str()?),
        None => sink.write("null"),
    }
}

fn sql_array<'a, F: FnMut(usize)>(
    array: &'a SqlArray,
    stack: &mut Stack<'a>,
    sink: &mut Sink<'_, F>,
) -> Result<(), QueryError> {
    if array.dimensions().is_empty() {
        return sink.write("[]");
    }
    // Public constructors enforce the shape. Invalid legacy/deserialized
    // values follow the existing nested-JSON fallback to an empty array.
    let mut reached_zero = false;
    for dimension in array.dimensions() {
        sink.controls.check_interruption()?;
        let Ok(length) = usize::try_from(dimension.length) else {
            return sink.write("[]");
        };
        if length == 0 {
            reached_zero = true;
            break;
        }
    }
    if !reached_zero {
        let mut expected = 1_usize;
        for dimension in array.dimensions() {
            sink.controls.check_interruption()?;
            let length = usize::try_from(dimension.length)
                .map_err(|_| CassieError::Execution("invalid SQL array shape".into()))?;
            if expected > array.elements().len() / length {
                return sink.write("[]");
            }
            expected *= length;
        }
    }
    let stride = if array.dimensions()[0].length == 0 {
        0
    } else {
        array_stride(&array.dimensions()[1..], sink.controls)?
    };
    sink.write("[")?;
    stack.push(Frame::SqlArray {
        array,
        depth: 0,
        next: 0,
        base: 0,
        stride,
    })
}

fn visit<'a, F: FnMut(usize)>(
    stack: &mut Stack<'a>,
    sink: &mut Sink<'_, F>,
) -> Result<(), QueryError> {
    while let Some(frame) = stack.frames.pop() {
        sink.controls.check_interruption()?;
        match frame {
            Frame::Json(value) => match value {
                serde_json::Value::Null => sink.write("null")?,
                serde_json::Value::Bool(value) => {
                    sink.write(if *value { "true" } else { "false" })?
                }
                serde_json::Value::Number(value) => sink.write(value.as_str())?,
                serde_json::Value::String(value) => sink.quoted(value)?,
                serde_json::Value::Array(values) => {
                    sink.write("[")?;
                    stack.push(Frame::JsonArray(values.iter(), false))?;
                }
                serde_json::Value::Object(values) => {
                    sink.write("{")?;
                    stack.push(Frame::JsonObject(values.iter(), false))?;
                }
            },
            Frame::JsonArray(mut values, emitted) => {
                if let Some(value) = values.next() {
                    if emitted {
                        sink.write(",")?;
                    }
                    stack.push(Frame::JsonArray(values, true))?;
                    stack.push(Frame::Json(value))?;
                } else {
                    sink.write("]")?;
                }
            }
            Frame::JsonObject(mut values, emitted) => {
                if let Some((key, value)) = values.next() {
                    if emitted {
                        sink.write(",")?;
                    }
                    sink.quoted(key)?;
                    sink.write(":")?;
                    stack.push(Frame::JsonObject(values, true))?;
                    stack.push(Frame::Json(value))?;
                } else {
                    sink.write("}")?;
                }
            }
            Frame::SqlValue(value) => match value {
                Value::Null => sink.write("null")?,
                Value::Bool(value) => sink.write(if *value { "true" } else { "false" })?,
                Value::Int64(value) => sink.write(fixed(value)?.as_str()?)?,
                Value::Float64(value) => float_json(*value, sink)?,
                Value::String(value) => sink.quoted(value)?,
                Value::Json(value) => stack.push(Frame::Json(value))?,
                Value::Array(value) => sql_array(value, stack, sink)?,
                Value::Typed(value) => typed(value, true, sink)?,
                Value::Vector(value) => {
                    sink.write("[")?;
                    let mut emitted = false;
                    for value in &value.values {
                        sink.controls.check_interruption()?;
                        if !value.is_finite() {
                            continue;
                        }
                        if emitted {
                            sink.write(",")?;
                        }
                        float_json(f64::from(*value), sink)?;
                        emitted = true;
                    }
                    sink.write("]")?;
                }
            },
            Frame::SqlArray {
                array,
                depth,
                next,
                base,
                stride,
            } => {
                let length = usize::try_from(array.dimensions()[depth].length)
                    .map_err(|_| CassieError::Execution("invalid SQL array shape".into()))?;
                if next == length {
                    sink.write("]")?;
                    continue;
                }
                if next > 0 {
                    sink.write(",")?;
                }
                stack.push(Frame::SqlArray {
                    array,
                    depth,
                    next: next + 1,
                    base,
                    stride,
                })?;
                if depth + 1 == array.dimensions().len() {
                    let offset = add(base, next)?;
                    stack.push(Frame::SqlValue(&array.elements()[offset]))?;
                } else {
                    let child_base = add(base, mul(next, stride)?)?;
                    let child_length = usize::try_from(array.dimensions()[depth + 1].length)
                        .map_err(|_| CassieError::Execution("invalid SQL array shape".into()))?;
                    let child_stride = if child_length == 0 {
                        0
                    } else {
                        stride / child_length
                    };
                    sink.write("[")?;
                    stack.push(Frame::SqlArray {
                        array,
                        depth: depth + 1,
                        next: 0,
                        base: child_base,
                        stride: child_stride,
                    })?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    static STRIDE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn array_stride(
    dimensions: &[crate::types::ArrayDimension],
    controls: &QueryExecutionControls,
) -> Result<usize, QueryError> {
    for dimension in dimensions {
        controls.check_interruption()?;
        #[cfg(test)]
        STRIDE_VISITS.with(|count| count.set(count.get().saturating_add(1)));
        let length = usize::try_from(dimension.length)
            .map_err(|_| CassieError::Execution("invalid SQL array shape".into()))?;
        if length == 0 {
            return Ok(0);
        }
    }
    let mut stride = 1;
    for dimension in dimensions {
        controls.check_interruption()?;
        #[cfg(test)]
        STRIDE_VISITS.with(|count| count.set(count.get().saturating_add(1)));
        stride = mul(
            stride,
            usize::try_from(dimension.length)
                .map_err(|_| CassieError::Execution("invalid SQL array shape".into()))?,
        )?;
    }
    Ok(stride)
}

pub(crate) fn serialize(
    row: &impl RowAccess,
    controls: &QueryExecutionControls,
) -> Result<TieText, QueryError> {
    serialize_with_probe(row, controls, None)
}

pub(super) fn serialize_with_probe<'a>(
    row: &impl RowAccess,
    controls: &'a QueryExecutionControls,
    probe: Option<&'a dyn Fn() -> Result<(), QueryError>>,
) -> Result<TieText, QueryError> {
    serialize_observing_with_probe(row, controls, probe, |_| {})
}
#[cfg(test)]
fn serialize_observing(
    row: &impl RowAccess,
    controls: &QueryExecutionControls,
    progress: impl FnMut(usize),
) -> Result<TieText, QueryError> {
    serialize_observing_with_probe(row, controls, None, progress)
}

fn serialize_observing_with_probe<'a>(
    row: &impl RowAccess,
    controls: &'a QueryExecutionControls,
    probe: Option<&'a dyn Fn() -> Result<(), QueryError>>,
    progress: impl FnMut(usize),
) -> Result<TieText, QueryError> {
    controls.check_interruption()?;
    let mut sink = Sink {
        text: String::new(),
        memory: controls.reserve_query_memory(0)?,
        controls,
        progress,
        since_check: 0,
        before_first_growth: probe,
    };
    let mut stack = Stack::new(controls)?;
    for (index, (_, value)) in row.entries().iter().enumerate() {
        controls.check_interruption()?;
        if index > 0 {
            sink.write("|")?;
        }
        match value {
            Value::Null => sink.write("<null>")?,
            Value::Bool(value) => sink.write(if *value { "true" } else { "false" })?,
            Value::Int64(value) => sink.write(fixed(value)?.as_str()?)?,
            Value::Float64(value) => sink.write(fixed(value)?.as_str()?)?,
            Value::String(value) => sink.write(value)?,
            Value::Vector(value) => {
                for (index, value) in value.values.iter().enumerate() {
                    controls.check_interruption()?;
                    if index > 0 {
                        sink.write(",")?;
                    }
                    sink.write(fixed(value)?.as_str()?)?;
                }
            }
            Value::Json(value) => {
                stack.push(Frame::Json(value))?;
                visit(&mut stack, &mut sink)?;
            }
            Value::Array(value) => {
                for dimension in value.dimensions() {
                    controls.check_interruption()?;
                    let upper = dimension
                        .lower_bound
                        .wrapping_add(dimension.length)
                        .wrapping_sub(1);
                    sink.write("[")?;
                    sink.write(fixed(dimension.lower_bound)?.as_str()?)?;
                    sink.write(":")?;
                    sink.write(fixed(upper)?.as_str()?)?;
                    sink.write("]")?;
                }
                sql_array(value, &mut stack, &mut sink)?;
                visit(&mut stack, &mut sink)?;
            }
            Value::Typed(value) => typed(value, false, &mut sink)?,
        }
    }
    drop(stack);
    if let Some(probe) = sink.before_first_growth.take() {
        probe()?;
    }
    Ok(TieText {
        text: sink.text,
        memory: sink.memory,
    })
}

#[cfg(test)]
fn growth_observing(
    text: &str,
    controls: &QueryExecutionControls,
    after_copy: impl FnMut(usize),
) -> Result<TieText, QueryError> {
    let mut sink = Sink {
        text: String::new(),
        memory: controls.reserve_query_memory(0)?,
        controls,
        progress: |_| {},
        since_check: 0,
        before_first_growth: None,
    };
    sink.write(text)?;
    sink.write_observing_growth("!", after_copy)?;
    Ok(TieText {
        text: sink.text,
        memory: sink.memory,
    })
}
