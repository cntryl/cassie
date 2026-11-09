//! Value-mode binary COPY JSON decoding to an owned compact document.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::OwnedJsonText;
use crate::runtime::QueryExecutionControls;

use super::super::forest::{output, Forest};
use super::super::slots::NodeId;
use super::{CursorError, Document, DocumentError, DocumentProgress, SourceStack, WrappedCause};

#[cfg(test)]
#[path = "document_binary_tests.rs"]
mod tests;

#[derive(Debug)]
pub(crate) enum BinaryJsonError {
    Invalid,
    Controlled(CassieError),
}

fn map_document_error(error: DocumentError) -> BinaryJsonError {
    match error {
        DocumentError::Control(error)
        | DocumentError::Token(CursorError::Interrupted(error))
        | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))) => {
            BinaryJsonError::Controlled(error)
        }
        DocumentError::Token(_)
        | DocumentError::Syntax(_, _)
        | DocumentError::NumberSentinel(_)
        | DocumentError::SentinelType(_)
        | DocumentError::Wrapped(_) => BinaryJsonError::Invalid,
    }
}

pub(crate) fn binary_json_text(
    source: &[u8],
    controls: &QueryExecutionControls,
) -> Result<OwnedJsonText, BinaryJsonError> {
    binary_json_text_with(source, controls, |_| {}, output::serialize)
}

#[cfg(test)]
fn binary_json_text_observed(
    source: &[u8],
    controls: &QueryExecutionControls,
    parser_observer: impl FnMut(DocumentProgress),
    output_observer: impl FnMut(output::OutputProgress),
) -> Result<OwnedJsonText, BinaryJsonError> {
    binary_json_text_with(source, controls, parser_observer, |forest, root| {
        output::serialize_observed(forest, root, output_observer)
    })
}

fn binary_json_text_with(
    source: &[u8],
    controls: &QueryExecutionControls,
    parser_observer: impl FnMut(DocumentProgress),
    serialize: impl FnOnce(&Forest, NodeId) -> Result<OwnedJsonText, CassieError>,
) -> Result<OwnedJsonText, BinaryJsonError> {
    let sources = SourceStack::from_bytes(source, controls)
        .map_err(|error| map_document_error(DocumentError::from(error)))?;
    let parsed =
        Document::parse_sources(sources, controls, parser_observer).map_err(map_document_error)?;
    serialize(&parsed.forest, parsed.root).map_err(BinaryJsonError::Controlled)
}
