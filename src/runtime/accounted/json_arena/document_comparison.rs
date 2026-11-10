//! Value-mode parsing for controlled JSONB comparison.
use super::{CursorError, Document, DocumentError, Parsed, SourceStack, WrappedCause};
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use std::cmp::Ordering;

fn parse(input: &str, controls: &QueryExecutionControls) -> Result<Option<Parsed>, CassieError> {
    controls.check_interruption()?;
    let sources =
        SourceStack::from_bytes(input.as_bytes(), controls).map_err(|error| match error {
            CursorError::Interrupted(error) => error,
            _ => CassieError::Execution("invalid UTF-8 in JSONB comparison operand".into()),
        })?;
    match Document::parse_sources(sources, controls, |_| {}) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(
            DocumentError::Control(error)
            | DocumentError::Token(CursorError::Interrupted(error))
            | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))),
        ) => Err(error),
        Err(_) => Ok(None),
    }
}

pub(crate) fn compare(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<Ordering>, CassieError> {
    let Some(left) = parse(left, controls)? else {
        return Ok(None);
    };
    let Some(right) = parse(right, controls)? else {
        return Ok(None);
    };
    super::super::forest::comparison::compare(
        &left.forest,
        left.root,
        &right.forest,
        right.root,
        controls,
    )
}
