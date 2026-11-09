//! Test-only schema-typed raw JSON cell; no production representation change.
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress, Sink};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{CursorError, Document, DocumentError, WrappedCause};

#[path = "document_cell_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Identity {
    Json,
    Jsonb,
}

#[derive(Debug)]
enum CellError {
    Syntax,
    Nul,
    Numeric,
    Control,
}

struct Cell {
    text: String,
    identity: Identity,
    memory: QueryMemoryReservation,
}

impl Cell {
    fn parse(
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
        mut copied: impl FnMut(),
    ) -> Result<Self, CellError> {
        let parsed = Document::parse_plain(source, controls).map_err(|error| match error {
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
                sink.write(source, &mut |progress| {
                    if matches!(progress, Progress::Copy) {
                        copied();
                    }
                })
                .map_err(|_| CellError::Control)?;
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

    fn text(&self) -> &str {
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
