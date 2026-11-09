//! Test-only schema-typed raw JSON cell; no production representation change.
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress, Sink};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{CursorError, Document, DocumentError, ObjectPolicy, SourceStack, WrappedCause};

#[path = "document_cell_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Identity {
    Json,
    Jsonb,
}

#[derive(Debug)]
pub(crate) enum CellError {
    Syntax,
    Utf8,
    Nul,
    Numeric,
    Control,
}

pub(crate) struct Cell {
    text: String,
    identity: Identity,
    memory: QueryMemoryReservation,
}

impl Cell {
    pub(crate) fn parse(
        source: &str,
        identity: Identity,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CellError> {
        Self::parse_observing(source, identity, controls, || {})
    }

    fn parse_observing(
        source: &str,
        identity: Identity,
        controls: &QueryExecutionControls,
        copied: impl FnMut(),
    ) -> Result<Self, CellError> {
        Self::parse_bytes_observing(source.as_bytes(), identity, controls, copied)
    }

    pub(crate) fn parse_bytes_observing(
        source: &[u8],
        identity: Identity,
        controls: &QueryExecutionControls,
        mut copied: impl FnMut(),
    ) -> Result<Self, CellError> {
        let sources = SourceStack::from_bytes(source, controls).map_err(|error| match error {
            CursorError::InvalidUtf8 => CellError::Utf8,
            _ => CellError::Control,
        })?;
        let parsed =
            Document::parse_sources_with_policy(sources, controls, ObjectPolicy::Plain, |_| {})
                .map_err(|error| match error {
                    DocumentError::Control(_)
                    | DocumentError::Token(CursorError::Interrupted(_))
                    | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(_))) => {
                        CellError::Control
                    }
                    _ => CellError::Syntax,
                })?;
        let output = match identity {
            Identity::Json => {
                let mut sink = Sink::for_arena(controls).map_err(|_| CellError::Control)?;
                let mut start = 0;
                while start < source.len() {
                    controls
                        .check_interruption()
                        .map_err(|_| CellError::Control)?;
                    let mut end = start.saturating_add(1024).min(source.len());
                    while end < source.len() && source[end] & 0xc0 == 0x80 {
                        end -= 1;
                    }
                    let chunk =
                        std::str::from_utf8(&source[start..end]).map_err(|_| CellError::Utf8)?;
                    sink.write(chunk, &mut |progress| {
                        if matches!(progress, Progress::Copy) {
                            copied();
                        }
                    })
                    .map_err(|_| CellError::Control)?;
                    start = end;
                }
                sink.finish_output()
            }
            Identity::Jsonb => {
                super::super::forest::jsonb_output::serialize(&parsed.forest, parsed.root).map_err(
                    |error| match error {
                        super::super::forest::jsonb_output::SerializeError::Nul => CellError::Nul,
                        super::super::forest::jsonb_output::SerializeError::NumericOverflow => {
                            CellError::Numeric
                        }
                        super::super::forest::jsonb_output::SerializeError::Control(_) => {
                            CellError::Control
                        }
                    },
                )?
            }
        };
        controls
            .check_interruption()
            .map_err(|_| CellError::Control)?;
        Self::from_output(output, identity, controls)
    }

    fn from_output(
        output: OwnedJsonText,
        identity: Identity,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CellError> {
        let (text, memory) = output.into_parts();
        let cell = Self {
            text,
            identity,
            memory,
        };
        if !cell.memory.belongs_to(controls) {
            return Err(CellError::Control);
        }
        Ok(cell)
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    fn identity(&self) -> Identity {
        self.identity
    }

    fn try_clone(&self, controls: &QueryExecutionControls) -> Result<Self, CellError> {
        self.try_clone_observing(controls, || {})
    }

    fn try_clone_observing(
        &self,
        controls: &QueryExecutionControls,
        mut copied: impl FnMut(),
    ) -> Result<Self, CellError> {
        if !self.memory.belongs_to(controls) {
            return Err(CellError::Control);
        }
        let mut sink = Sink::for_arena(controls).map_err(|_| CellError::Control)?;
        sink.write(&self.text, &mut |progress| {
            if matches!(progress, Progress::Copy) {
                copied();
            }
        })
        .map_err(|_| CellError::Control)?;
        Self::from_output(sink.finish_output(), self.identity, controls)
    }
}
