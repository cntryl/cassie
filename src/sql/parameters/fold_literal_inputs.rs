//! Validate reached type-function constants before result metadata is published.
use super::{Fold, ParameterAdmissionError};
use crate::types::DataType;

impl Fold<'_, '_> {
    pub(super) fn validate_type_function_literal(
        &self,
        input: &str,
        target: &DataType,
    ) -> Result<(), ParameterAdmissionError> {
        let diagnostic = crate::sql::literal_inputs::diagnostic(input, target);
        if let Some(diagnostic) = diagnostic {
            self.occurrences
                .invalid_scalar_literal(input, diagnostic, self.controls)?;
        }
        Ok(())
    }
}
