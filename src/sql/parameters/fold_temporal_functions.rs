//! Narrow temporal function diagnostics during controlled parameter folding.

use super::{Fold, Frame, ParameterAdmissionError};
use crate::sql::ast::FunctionCall;

impl Fold<'_, '_> {
    pub(super) fn reject_timetz_to_char(
        &mut self,
        function: &FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<bool, ParameterAdmissionError> {
        let first_oid = self
            .summaries
            .values
            .get(frame.start)
            .map(|summary| summary.oid);
        let selected = crate::sql::timetz_format_signature::candidate(function, |slot| {
            first_oid == Some(1266) && self.occurrences.original_wire_oid(slot) == Some(1266)
        });
        if let Some(qualified) = selected {
            self.occurrences.to_char_timetz(qualified)?;
            return Ok(true);
        }
        Ok(false)
    }
}
