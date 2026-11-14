//! Parameter admission of the BYTEA overloads of the text length and slice
//! builtins: `length(bytea)` and `octet_length(bytea)` return `integer`, and
//! `substring(bytea, integer [, integer])` returns `bytea`.

use crate::sql::{ast, functions};

use super::{check_controls, Fold, Frame, ParameterAdmissionError, Summary};

const BYTEA_OID: i32 = 17;

impl Fold<'_, '_> {
    /// The result of a BYTEA overload, or `None` when the first argument is
    /// not `bytea` and the text signature applies.
    pub(super) fn bytea_overload(
        &mut self,
        function: &ast::FunctionCall,
        metadata: &functions::ScalarFunction,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let measures = matches!(metadata.name, "length" | "octet_length");
        let slices = matches!(metadata.name, "substring" | "substr");
        if !(measures || slices)
            || self
                .summaries
                .values
                .get(frame.start)
                .map(|summary| summary.oid)
                != Some(BYTEA_OID)
        {
            return Ok(None);
        }
        if measures {
            return Ok(Some(Summary::known(23)));
        }
        for summary in self.summaries.values[frame.start + 1..]
            .iter_mut()
            .take(function.args.len().saturating_sub(1))
        {
            check_controls(self.controls)?;
            self.occurrences.coerce(summary, 23, true)?;
        }
        Ok(Some(Summary::known(BYTEA_OID)))
    }
}
