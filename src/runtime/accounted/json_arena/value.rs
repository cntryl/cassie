//! Ordinary JSON parsing into the existing public `serde_json::Value` carrier.
use crate::app::CassieError;
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;

use super::cursor::CursorError;
use super::document::{Document, DocumentError, ObjectPolicy, WrappedCause};
use super::sources::SourceStack;

#[cfg(test)]
#[path = "value/tests.rs"]
mod tests;

#[derive(Debug)]
pub(crate) enum ValueParseError {
    Invalid,
    Controlled(CassieError),
}

pub(crate) fn parse_value(
    input: &[u8],
    controls: &QueryExecutionControls,
) -> Result<Accounted<serde_json::Value>, ValueParseError> {
    controls
        .check_interruption()
        .map_err(ValueParseError::Controlled)?;
    let retained_bytes = super::super::json::serialized_decode_bytes(input.len().max(1))
        .map_err(ValueParseError::Controlled)?;
    let reservation = controls
        .reserve_query_memory(retained_bytes)
        .map_err(ValueParseError::Controlled)?;
    let value = parse_value_in_reserved_memory(input, controls)?;
    Ok(Accounted::from_admitted(value, reservation))
}

pub(crate) fn parse_value_in_reserved_memory(
    input: &[u8],
    controls: &QueryExecutionControls,
) -> Result<serde_json::Value, ValueParseError> {
    controls
        .check_interruption()
        .map_err(ValueParseError::Controlled)?;
    let sources = SourceStack::from_bytes(input, controls).map_err(map_source_error)?;
    let parsed =
        Document::parse_sources_with_policy(sources, controls, ObjectPolicy::Plain, |_| {})
            .map_err(map_document_error)?;
    let value = parsed
        .forest
        .to_serde_value(parsed.root)
        .map_err(ValueParseError::Controlled)?;
    controls
        .check_interruption()
        .map_err(ValueParseError::Controlled)?;
    drop(parsed);
    Ok(value)
}

pub(crate) fn parse_value_with_optional_controls(
    input: &[u8],
    controls: Option<&QueryExecutionControls>,
) -> Result<Accounted<serde_json::Value>, ValueParseError> {
    if let Some(controls) = controls {
        return parse_value(input, controls);
    }
    let limits = crate::config::CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    parse_value(input, &controls)
}

pub(crate) fn syntax_error(input: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(input)
        .err()
        .map_or_else(
            || "invalid ordinary JSON".to_string(),
            |error| error.to_string(),
        )
}

fn map_source_error(error: CursorError) -> ValueParseError {
    match error {
        CursorError::Interrupted(error) => ValueParseError::Controlled(error),
        _ => ValueParseError::Invalid,
    }
}

fn map_document_error(error: DocumentError) -> ValueParseError {
    match error {
        DocumentError::Control(error)
        | DocumentError::Token(CursorError::Interrupted(error))
        | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))) => {
            ValueParseError::Controlled(error)
        }
        _ => ValueParseError::Invalid,
    }
}
