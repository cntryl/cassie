//! Owned Value-mode JSON array whose strings can be borrowed as path elements.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;

use super::super::forest::TextArrayElement;
use super::{
    CursorError, Document, DocumentError, DocumentProgress, Parsed, SourceStack, WrappedCause,
};

#[cfg(test)]
#[path = "document_text_array_tests.rs"]
mod tests;

pub(crate) struct OwnedTextArray {
    parsed: Parsed,
    length: usize,
}

impl OwnedTextArray {
    pub(crate) fn len(&self) -> usize {
        self.length
    }
    pub(crate) fn get(&self, index: usize) -> Result<Option<&str>, CassieError> {
        if index >= self.length {
            return Err(CassieError::Execution(
                "invalid JSON text-array index".into(),
            ));
        }
        match self
            .parsed
            .forest
            .text_array_element(self.parsed.root, index)?
        {
            TextArrayElement::Text(text) => Ok(Some(text)),
            TextArrayElement::Null => Ok(None),
            TextArrayElement::Other => Err(CassieError::Execution(
                "invalid JSON text-array element".into(),
            )),
        }
    }
}

pub(crate) fn parse(
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<OwnedTextArray>, CassieError> {
    parse_observing(input, controls, || {})
}

fn parse_observing(
    input: &str,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(),
) -> Result<Option<OwnedTextArray>, CassieError> {
    controls.check_interruption()?;
    let trimmed = trim(input, controls)?;
    if !trimmed.starts_with('[') {
        return Ok(None);
    }
    let sources =
        SourceStack::from_bytes(trimmed.as_bytes(), controls).map_err(|error| match error {
            CursorError::Interrupted(error) => error,
            _ => CassieError::Execution("invalid UTF-8 in JSON text-array operand".into()),
        })?;
    let parsed = match Document::parse_sources(sources, controls, |progress| {
        if matches!(progress, DocumentProgress::FramePushed) {
            observe();
        }
    }) {
        Ok(parsed) => parsed,
        Err(
            DocumentError::Control(error)
            | DocumentError::Token(CursorError::Interrupted(error))
            | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))),
        ) => return Err(error),
        Err(_) => return Ok(None),
    };
    let Some(length) = parsed.forest.text_array_len(parsed.root)? else {
        return Ok(None);
    };
    if i32::try_from(length).is_err() {
        return Ok(None);
    }
    for index in 0..length {
        controls.check_interruption()?;
        if matches!(
            parsed.forest.text_array_element(parsed.root, index)?,
            TextArrayElement::Other
        ) {
            return Ok(None);
        }
    }
    Ok(Some(OwnedTextArray { parsed, length }))
}

fn trim<'a>(input: &'a str, controls: &QueryExecutionControls) -> Result<&'a str, CassieError> {
    let mut start = 0;
    for (index, character) in input.char_indices() {
        controls.check_interruption()?;
        if !character.is_whitespace() {
            break;
        }
        start = index + character.len_utf8();
    }
    let mut end = input.len();
    for (index, character) in input[start..].char_indices().rev() {
        controls.check_interruption()?;
        if !character.is_whitespace() {
            break;
        }
        end = start + index;
    }
    Ok(&input[start..end])
}
