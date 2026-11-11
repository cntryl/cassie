use crate::sql::ast;

use super::{check_controls, Fold, Frame, ParameterAdmissionError, Summary};

impl Fold<'_, '_> {
    pub(super) fn digest_signature_output(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        let summaries = &mut self.summaries.values[frame.start..];
        if let Some(first) = summaries.first_mut() {
            check_controls(self.controls)?;
            match first.oid {
                17 => {}
                0 | 25 | 1042 | 1043 | 705 => {
                    self.occurrences.coerce(first, 25, true)?;
                }
                oid => {
                    self.occurrences.undefined_function(
                        "digest",
                        function.args.len(),
                        Some(oid),
                    )?;
                    return Ok(Summary::unqualified());
                }
            }
        }
        if let Some(algorithm) = summaries.get_mut(1) {
            check_controls(self.controls)?;
            self.occurrences.coerce(algorithm, 25, true)?;
        }
        Ok(Summary::known(17))
    }
}
