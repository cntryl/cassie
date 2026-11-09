//! Bounded, owned rendering of structured JSONB COPY parse diagnostics.
use std::fmt::{self, Write};

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{CursorError, DocumentError, Position, SyntaxKind, WrappedCause};

#[cfg(test)]
#[path = "document_diagnostic_tests.rs"]
mod tests;

#[derive(Debug)]
pub(super) enum DiagnosticError {
    Control(CassieError),
    NotSemantic,
}

impl From<CassieError> for DiagnosticError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

pub(crate) struct OwnedDiagnostic {
    text: String,
    _memory: QueryMemoryReservation,
}

impl fmt::Debug for OwnedDiagnostic {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.debug_tuple("OwnedDiagnostic")
            .field(&self.text)
            .finish()
    }
}

impl OwnedDiagnostic {
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }
}

// Every string below is a fixed literal. The only formatted values are a
// column ordinal, optional i64/u64 value, and two usize positions. Even on a
// 128-bit usize, their combined decimal widths and longest message fit 512.
struct StackText {
    bytes: [u8; 512],
    length: usize,
}

impl Write for StackText {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.length.checked_add(value.len()).ok_or(fmt::Error)?;
        let destination = self.bytes.get_mut(self.length..end).ok_or(fmt::Error)?;
        destination.copy_from_slice(value.as_bytes());
        self.length = end;
        Ok(())
    }
}

pub(super) fn render(
    error: &DocumentError,
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<OwnedDiagnostic, DiagnosticError> {
    controls.check_interruption()?;
    let mut stack = StackText {
        bytes: [0; 512],
        length: 0,
    };
    write!(
        &mut stack,
        "COPY binary field {column} is invalid: invalid input syntax for type jsonb: "
    )
    .map_err(overflow)?;
    write_document(error, &mut stack)?;
    finish(&stack, controls)
}

pub(super) enum FixedError {
    Missing,
    Version(u8),
    Utf8,
    Numeric,
    Nul,
}

impl fmt::Display for FixedError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => out.write_str("invalid binary jsonb value"),
            Self::Version(version) => write!(out, "unsupported jsonb version number {version}"),
            Self::Utf8 => out.write_str("invalid byte sequence for encoding UTF8"),
            Self::Numeric => out.write_str("value overflows numeric format"),
            Self::Nul => out.write_str("unsupported Unicode escape sequence: \\u0000"),
        }
    }
}

pub(super) fn render_fixed(
    message: FixedError,
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<OwnedDiagnostic, DiagnosticError> {
    controls.check_interruption()?;
    let mut stack = StackText {
        bytes: [0; 512],
        length: 0,
    };
    write!(
        &mut stack,
        "COPY binary field {column} is invalid: {message}"
    )
    .map_err(overflow)?;
    finish(&stack, controls)
}

fn finish(
    stack: &StackText,
    controls: &QueryExecutionControls,
) -> Result<OwnedDiagnostic, DiagnosticError> {
    controls.check_interruption()?;
    let source =
        std::str::from_utf8(&stack.bytes[..stack.length]).map_err(|_| overflow(fmt::Error))?;
    let memory = controls.reserve_query_memory(source.len())?;
    let mut text = String::new();
    text.try_reserve_exact(source.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > source.len() {
        return Err(overflow(fmt::Error));
    }
    text.push_str(source);
    controls.check_interruption()?;
    Ok(OwnedDiagnostic {
        text,
        _memory: memory,
    })
}

fn write_document(error: &DocumentError, out: &mut impl Write) -> Result<(), DiagnosticError> {
    match error {
        DocumentError::Control(_) => return Err(DiagnosticError::NotSemantic),
        DocumentError::Token(error) => write_token(error, out)?,
        DocumentError::Syntax(kind, position) => {
            write_syntax(*kind, *position, out).map_err(overflow)?
        }
        DocumentError::NumberSentinel(error) => error.write_message(out).map_err(overflow)?,
        DocumentError::SentinelType(error) => error.write_message(out).map_err(overflow)?,
        DocumentError::Wrapped(cause) => match cause {
            WrappedCause::Syntax(kind, position) => {
                write_syntax(*kind, *position, out).map_err(overflow)?
            }
            WrappedCause::Token(error) => write_token(error, out)?,
            WrappedCause::Number(error) => error.write_message(out).map_err(overflow)?,
            WrappedCause::Type(error) => error.write_message(out).map_err(overflow)?,
        },
    }
    Ok(())
}

fn write_token(error: &CursorError, out: &mut impl Write) -> Result<(), DiagnosticError> {
    match error {
        CursorError::InvalidUtf8 | CursorError::Interrupted(_) => {
            return Err(DiagnosticError::NotSemantic)
        }
        CursorError::Syntax(error) => error.write_message(out),
        CursorError::Number(error) => error.write_message(out),
        CursorError::Literal(error) => error.write_message(out),
    }
    .map_err(overflow)
}

fn write_syntax(kind: SyntaxKind, position: Position, out: &mut impl Write) -> fmt::Result {
    let message = match kind {
        SyntaxKind::EofValue => "EOF while parsing a value",
        SyntaxKind::EofList => "EOF while parsing a list",
        SyntaxKind::EofObject => "EOF while parsing an object",
        SyntaxKind::ExpectedValue => "expected value",
        SyntaxKind::ExpectedListEnd => "expected `,` or `]`",
        SyntaxKind::ExpectedObjectEnd => "expected `,` or `}`",
        SyntaxKind::ExpectedKey => "key must be a string",
        SyntaxKind::ExpectedColon => "expected `:`",
        SyntaxKind::TrailingComma => "trailing comma",
        SyntaxKind::TrailingCharacters => "trailing characters",
        SyntaxKind::Recursion => "recursion limit exceeded",
    };
    write!(
        out,
        "{message} at line {} column {}",
        position.line, position.column
    )
}

fn overflow(_: fmt::Error) -> DiagnosticError {
    DiagnosticError::Control(CassieError::ResourceLimit(
        "JSONB diagnostic allocation exceeded its bound".into(),
    ))
}
