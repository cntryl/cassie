//! Test-only schema-typed raw JSON cell; no production representation change.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress, Sink};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::diagnostic::{self, DiagnosticError, OwnedDiagnostic};
use super::{CursorError, Document, DocumentError, WrappedCause};

#[path = "document_cell_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Identity {
    Json,
    Jsonb,
}

#[derive(Debug)]
pub(crate) enum CellError {
    Syntax(OwnedDiagnostic),
    Utf8,
    Nul,
    Numeric,
    Control(CassieError),
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
        let parsed = match Document::parse_plain_row(source, controls) {
            Ok(parsed) => parsed,
            Err(
                DocumentError::Control(error)
                | DocumentError::Token(CursorError::Interrupted(error))
                | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))),
            ) => {
                return Err(CellError::Control(error));
            }
            Err(error) => {
                return Err(match diagnostic::render_row(&error, controls) {
                    Ok(diagnostic) => CellError::Syntax(diagnostic),
                    Err(DiagnosticError::Control(error)) => CellError::Control(error),
                    Err(DiagnosticError::NotSemantic) => CellError::Control(
                        CassieError::Execution("unexpected row JSON diagnostic".into()),
                    ),
                });
            }
        };
        let output = match identity {
            Identity::Json => {
                let mut sink = Sink::for_arena(controls).map_err(CellError::Control)?;
                let mut start = 0;
                while start < source.len() {
                    controls.check_interruption().map_err(CellError::Control)?;
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
                    .map_err(CellError::Control)?;
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
                        super::super::forest::jsonb_output::SerializeError::Control(error) => {
                            CellError::Control(error)
                        }
                    },
                )?
            }
        };
        controls.check_interruption().map_err(CellError::Control)?;
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
            return Err(CellError::Control(CassieError::ResourceLimit(
                "raw JSON cell uses a different query tracker".into(),
            )));
        }
        Ok(cell)
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn identity(&self) -> Identity {
        self.identity
    }

    pub(crate) fn belongs_to(&self, controls: &QueryExecutionControls) -> bool {
        self.memory.belongs_to(controls)
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
            return Err(CellError::Control(CassieError::ResourceLimit(
                "raw JSON cell uses a different query tracker".into(),
            )));
        }
        let mut sink = Sink::for_arena(controls).map_err(CellError::Control)?;
        sink.write(&self.text, &mut |progress| {
            if matches!(progress, Progress::Copy) {
                copied();
            }
        })
        .map_err(CellError::Control)?;
        Self::from_output(sink.finish_output(), self.identity, controls)
    }
}
