//! Isolated scalar binary JSONB decode-to-text pipeline.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::OwnedJsonText;
use crate::runtime::QueryExecutionControls;

use super::super::forest::jsonb_output::{self, SerializeError};
use super::diagnostic::{self, DiagnosticError, FixedError, OwnedDiagnostic};
use super::{CursorError, Document, DocumentError, SourceStack, WrappedCause};

#[cfg(test)]
#[path = "document_jsonb_tests.rs"]
mod tests;

pub(crate) enum JsonbFieldError {
    Diagnostic(OwnedDiagnostic),
    Control(CassieError),
}

pub(crate) enum JsonbTextError {
    Invalid,
    NumericOverflow,
    Nul,
    Control(CassieError),
}

impl std::fmt::Debug for JsonbFieldError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Diagnostic(_) => out.write_str("JsonbFieldError::Diagnostic"),
            Self::Control(error) => out
                .debug_tuple("JsonbFieldError::Control")
                .field(error)
                .finish(),
        }
    }
}

#[cfg(test)]
impl JsonbFieldError {
    fn message(&self) -> Option<&str> {
        match self {
            Self::Diagnostic(error) => Some(error.as_str()),
            Self::Control(_) => None,
        }
    }
}

pub(crate) fn parse_field(
    source: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<OwnedJsonText, JsonbFieldError> {
    controls
        .check_interruption()
        .map_err(JsonbFieldError::Control)?;
    let Some((&version, body)) = source.split_first() else {
        return Err(fixed(&FixedError::Missing, column, controls));
    };
    if version != 1 {
        return Err(fixed(&FixedError::Version(version), column, controls));
    }
    let sources = SourceStack::from_bytes(body, controls).map_err(|error| match error {
        CursorError::InvalidUtf8 => fixed(&FixedError::Utf8, column, controls),
        CursorError::Interrupted(error) => JsonbFieldError::Control(error),
        _ => JsonbFieldError::Control(CassieError::Execution(
            "unexpected JSONB source error".into(),
        )),
    })?;
    let parsed = Document::parse_sources_plain(sources, controls, |_| {})
        .map_err(|error| document_error(error, column, controls))?;
    jsonb_output::serialize(&parsed.forest, parsed.root).map_err(|error| match error {
        SerializeError::Control(error) => JsonbFieldError::Control(error),
        SerializeError::NumericOverflow => fixed(&FixedError::Numeric, column, controls),
        SerializeError::Nul => fixed(&FixedError::Nul, column, controls),
    })
}

pub(crate) fn canonical_text(
    input: &[u8],
    controls: &QueryExecutionControls,
) -> Result<OwnedJsonText, JsonbTextError> {
    controls
        .check_interruption()
        .map_err(JsonbTextError::Control)?;
    let sources = SourceStack::from_bytes(input, controls).map_err(|error| match error {
        CursorError::Interrupted(error) => JsonbTextError::Control(error),
        _ => JsonbTextError::Invalid,
    })?;
    let parsed =
        Document::parse_sources_plain(sources, controls, |_| {}).map_err(|error| match error {
            DocumentError::Control(error)
            | DocumentError::Token(CursorError::Interrupted(error))
            | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))) => {
                JsonbTextError::Control(error)
            }
            _ => JsonbTextError::Invalid,
        })?;
    jsonb_output::serialize(&parsed.forest, parsed.root).map_err(|error| match error {
        SerializeError::Control(error) => JsonbTextError::Control(error),
        SerializeError::NumericOverflow => JsonbTextError::NumericOverflow,
        SerializeError::Nul => JsonbTextError::Nul,
    })
}

// Array fallback uses this only after the same immutable field has already
// completed JSONB canonical validation in its envelope pass.
pub(in crate::runtime::accounted::json_arena) fn reparse_compact_field(
    source: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<OwnedJsonText, JsonbFieldError> {
    controls
        .check_interruption()
        .map_err(JsonbFieldError::Control)?;
    let Some((&version, body)) = source.split_first() else {
        return Err(fixed(&FixedError::Missing, column, controls));
    };
    if version != 1 {
        return Err(fixed(&FixedError::Version(version), column, controls));
    }
    let sources = SourceStack::from_bytes(body, controls).map_err(|error| match error {
        CursorError::InvalidUtf8 => fixed(&FixedError::Utf8, column, controls),
        CursorError::Interrupted(error) => JsonbFieldError::Control(error),
        _ => JsonbFieldError::Control(CassieError::Execution(
            "unexpected JSONB source error".into(),
        )),
    })?;
    let parsed = Document::parse_sources_plain(sources, controls, |_| {})
        .map_err(|error| document_error(error, column, controls))?;
    super::super::forest::output::serialize(&parsed.forest, parsed.root)
        .map_err(JsonbFieldError::Control)
}

fn document_error(
    error: DocumentError,
    column: usize,
    controls: &QueryExecutionControls,
) -> JsonbFieldError {
    match error {
        DocumentError::Control(error)
        | DocumentError::Token(CursorError::Interrupted(error))
        | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))) => {
            JsonbFieldError::Control(error)
        }
        DocumentError::Token(CursorError::InvalidUtf8) => {
            fixed(&FixedError::Utf8, column, controls)
        }
        other => match diagnostic::render(&other, column, controls) {
            Ok(error) => JsonbFieldError::Diagnostic(error),
            Err(DiagnosticError::Control(error)) => JsonbFieldError::Control(error),
            Err(DiagnosticError::NotSemantic) => JsonbFieldError::Control(CassieError::Execution(
                "unexpected JSONB diagnostic".into(),
            )),
        },
    }
}

fn fixed(error: &FixedError, column: usize, controls: &QueryExecutionControls) -> JsonbFieldError {
    match diagnostic::render_fixed(error, column, controls) {
        Ok(error) => JsonbFieldError::Diagnostic(error),
        Err(DiagnosticError::Control(error)) => JsonbFieldError::Control(error),
        Err(DiagnosticError::NotSemantic) => {
            JsonbFieldError::Control(CassieError::Execution("unexpected JSONB diagnostic".into()))
        }
    }
}
