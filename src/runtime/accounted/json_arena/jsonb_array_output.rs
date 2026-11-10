//! Isolated JSONB-array rendering after ordered envelope validation.
use std::fmt::Write;

use crate::app::CassieError;
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress as SinkProgress, Sink};
use crate::runtime::QueryExecutionControls;

use super::document::diagnostic::{render_fixed, DiagnosticError, FixedError, OwnedDiagnostic};
use super::document::jsonb::{parse_field, reparse_compact_field, JsonbFieldError};
use super::jsonb_array_envelope::{
    decode, DecodeError, Element, Envelope, Progress as EnvelopeProgress,
};

#[cfg(test)]
#[path = "jsonb_array_output_tests.rs"]
mod tests;

pub(crate) enum OutputError {
    Diagnostic(OwnedDiagnostic),
    Control(CassieError),
}

impl std::fmt::Debug for OutputError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Diagnostic(_) => out.write_str("JSONB array diagnostic"),
            Self::Control(error) => out.debug_tuple("Control").field(error).finish(),
        }
    }
}

impl From<CassieError> for OutputError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

impl From<JsonbFieldError> for OutputError {
    fn from(error: JsonbFieldError) -> Self {
        match error {
            JsonbFieldError::Diagnostic(error) => Self::Diagnostic(error),
            JsonbFieldError::Control(error) => Self::Control(error),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Progress {
    ValidatedElement,
    OutputCopy,
    EscapeScan,
}

pub(crate) fn render(
    source: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<OwnedJsonText, OutputError> {
    render_with_observer(source, column, controls, |_| {})
}

#[cfg(test)]
fn render_observed(
    source: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    observer: impl FnMut(Progress),
) -> Result<OwnedJsonText, OutputError> {
    render_with_observer(source, column, controls, observer)
}

fn render_with_observer(
    source: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    mut observer: impl FnMut(Progress),
) -> Result<OwnedJsonText, OutputError> {
    let envelope = decode(
        source,
        controls,
        |field| {
            let validated = parse_field(field, column, controls)?;
            Ok::<_, JsonbFieldError>(validated.as_str() == "null")
        },
        |event| match event {
            EnvelopeProgress::Element => observer(Progress::ValidatedElement),
        },
    )
    .map_err(|error| match error {
        DecodeError::InvalidArray => fixed(FixedError::ArrayInvalid, column, controls),
        DecodeError::DocumentNull => fixed(FixedError::ArrayDocumentNull, column, controls),
        DecodeError::Element(error) => error.into(),
        DecodeError::Control(error) => OutputError::Control(error),
    })?;
    let mut sink = Sink::for_arena(controls)?;
    let Dimensions {
        lengths,
        lowers,
        count,
        nonstandard,
    } = dimensions(&envelope)?;
    if count == 0 {
        write_text(&mut sink, "{}", &mut observer)?;
        return Ok(sink.finish_output());
    }
    let compact = envelope.compact_fallback();
    if nonstandard && !compact {
        for index in 0..count {
            let length = i32::try_from(lengths[index])
                .map_err(|_| CassieError::Execution("invalid validated JSONB dimension".into()))?;
            let upper = lowers[index]
                .checked_add(length)
                .and_then(|value| value.checked_sub(1))
                .ok_or_else(|| CassieError::Execution("invalid validated JSONB bounds".into()))?;
            let mut bound = StackText::new();
            write!(&mut bound, "[{}:{}]", lowers[index], upper)
                .map_err(|_| CassieError::ResourceLimit("JSONB array bounds overflow".into()))?;
            write_text(&mut sink, bound.as_str()?, &mut observer)?;
        }
        write_text(&mut sink, "=", &mut observer)?;
    }
    let mut indices = [0_usize; 6];
    let mut opened = [false; 6];
    let mut depth = 0_usize;
    let mut element = 0_usize;
    loop {
        controls.check_interruption()?;
        if !opened[depth] {
            write_text(&mut sink, if compact { "[" } else { "{" }, &mut observer)?;
            opened[depth] = true;
        }
        if indices[depth] == lengths[depth] {
            write_text(&mut sink, if compact { "]" } else { "}" }, &mut observer)?;
            if depth == 0 {
                break;
            }
            opened[depth] = false;
            indices[depth] = 0;
            depth -= 1;
            continue;
        }
        if indices[depth] != 0 {
            write_text(&mut sink, ",", &mut observer)?;
        }
        indices[depth] += 1;
        if depth + 1 < count {
            depth += 1;
            continue;
        }
        match envelope
            .element(element)
            .ok_or_else(|| CassieError::Execution("missing validated JSONB element".into()))?
        {
            Element::SqlNull => write_text(
                &mut sink,
                if compact { "null" } else { "NULL" },
                &mut observer,
            )?,
            Element::Value(field) => {
                let text = if compact {
                    reparse_compact_field(field, column, controls)?
                } else {
                    parse_field(field, column, controls)?
                };
                if compact {
                    write_text(&mut sink, text.as_str(), &mut observer)?;
                } else {
                    write_array_element(&mut sink, text.as_str(), controls, &mut observer)?;
                }
            }
        }
        element = element
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("JSONB array position overflow".into()))?;
    }
    Ok(sink.finish_output())
}

struct Dimensions {
    lengths: [usize; 6],
    lowers: [i32; 6],
    count: usize,
    nonstandard: bool,
}

fn dimensions(envelope: &Envelope<'_>) -> Result<Dimensions, CassieError> {
    let mut dimensions = Dimensions {
        lengths: [0; 6],
        lowers: [0; 6],
        count: 0,
        nonstandard: false,
    };
    for (length, lower) in envelope.dimensions() {
        dimensions.lengths[dimensions.count] = usize::try_from(length)
            .map_err(|_| CassieError::Execution("invalid validated JSONB dimension".into()))?;
        dimensions.lowers[dimensions.count] = lower;
        dimensions.nonstandard |= lower != 1;
        dimensions.count += 1;
    }
    Ok(dimensions)
}

fn write_text(
    sink: &mut Sink<'_>,
    text: &str,
    observer: &mut impl FnMut(Progress),
) -> Result<(), CassieError> {
    sink.write(text, &mut |event| {
        if matches!(event, SinkProgress::Copy) {
            observer(Progress::OutputCopy);
        }
    })
}

fn write_array_element(
    sink: &mut Sink<'_>,
    text: &str,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(Progress),
) -> Result<(), CassieError> {
    let mut quote = text.is_empty() || (text.len() == 4 && text.eq_ignore_ascii_case("NULL"));
    let mut last = 0;
    for (offset, character) in text.char_indices() {
        if offset - last >= 1024 {
            observer(Progress::EscapeScan);
            controls.check_interruption()?;
            last = offset;
        }
        if character.is_whitespace() || matches!(character, '{' | '}' | ',' | '"' | '\\') {
            quote = true;
            break;
        }
    }
    if !quote {
        return write_text(sink, text, observer);
    }
    write_text(sink, "\"", observer)?;
    let mut start = 0;
    for (offset, character) in text.char_indices() {
        if offset - start >= 1024 {
            write_text(sink, &text[start..offset], observer)?;
            observer(Progress::EscapeScan);
            controls.check_interruption()?;
            start = offset;
        }
        if matches!(character, '"' | '\\') {
            write_text(sink, &text[start..offset], observer)?;
            write_text(sink, "\\", observer)?;
            start = offset;
        }
    }
    write_text(sink, &text[start..], observer)?;
    write_text(sink, "\"", observer)
}

fn fixed(error: FixedError, column: usize, controls: &QueryExecutionControls) -> OutputError {
    match render_fixed(&error, column, controls) {
        Ok(error) => OutputError::Diagnostic(error),
        Err(DiagnosticError::Control(error)) => OutputError::Control(error),
        Err(DiagnosticError::NotSemantic) => OutputError::Control(CassieError::Execution(
            "unexpected JSONB array diagnostic".into(),
        )),
    }
}

struct StackText {
    bytes: [u8; 32],
    length: usize,
}
impl StackText {
    const fn new() -> Self {
        Self {
            bytes: [0; 32],
            length: 0,
        }
    }
    fn as_str(&self) -> Result<&str, CassieError> {
        std::str::from_utf8(&self.bytes[..self.length])
            .map_err(|_| CassieError::Execution("invalid JSONB bounds".into()))
    }
}
impl Write for StackText {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let end = self
            .length
            .checked_add(value.len())
            .ok_or(std::fmt::Error)?;
        self.bytes
            .get_mut(self.length..end)
            .ok_or(std::fmt::Error)?
            .copy_from_slice(value.as_bytes());
        self.length = end;
        Ok(())
    }
}
