//! Owned Value-mode document used for checked top-level existence lookup.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;

use super::{
    CursorError, Document, DocumentError, DocumentProgress, Parsed, SourceStack, WrappedCause,
};

#[cfg(test)]
#[path = "document_exists_tests.rs"]
mod tests;

#[cfg(test)]
thread_local! {
    static CANCEL_EXISTENCE_PARSE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) fn cancel_existence_parse_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_EXISTENCE_PARSE.with(|slot| *slot.borrow_mut() = Some(handle));
}

pub(crate) struct OwnedExistence {
    parsed: Parsed,
}

impl OwnedExistence {
    pub(crate) fn object_keys(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Option<super::super::OwnedObjectKeys>, CassieError> {
        super::super::forest::object_keys::build(&self.parsed.forest, self.parsed.root, controls)
    }

    pub(crate) fn inspection_root(
        &self,
    ) -> Result<super::super::forest::InspectionRoot, CassieError> {
        self.parsed.forest.inspection_root(self.parsed.root)
    }

    pub(crate) fn contains(&self, key: &str) -> Result<bool, CassieError> {
        self.parsed.forest.key_exists(self.parsed.root, key)
    }
}

pub(crate) fn parse(
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<OwnedExistence>, CassieError> {
    parse_observing(input, controls, || {})
}

fn parse_observing(
    input: &str,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(),
) -> Result<Option<OwnedExistence>, CassieError> {
    controls.check_interruption()?;
    let sources =
        SourceStack::from_bytes(input.as_bytes(), controls).map_err(|error| match error {
            CursorError::Interrupted(error) => error,
            _ => CassieError::Execution("invalid UTF-8 in JSON existence operand".into()),
        })?;
    match Document::parse_sources(sources, controls, |progress| {
        if matches!(progress, DocumentProgress::FramePushed) {
            observe();
            #[cfg(test)]
            CANCEL_EXISTENCE_PARSE.with(|slot| {
                if let Some(handle) = slot.borrow_mut().take() {
                    handle.cancel();
                }
            });
        }
    }) {
        Ok(parsed) => Ok(Some(OwnedExistence { parsed })),
        Err(
            DocumentError::Control(error)
            | DocumentError::Token(CursorError::Interrupted(error))
            | DocumentError::Wrapped(WrappedCause::Token(CursorError::Interrupted(error))),
        ) => Err(error),
        Err(_) => Ok(None),
    }
}
