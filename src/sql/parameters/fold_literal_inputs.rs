//! Validate reached type-function constants before result metadata is published.
use super::super::diagnostics::ScalarLiteralDiagnostic;
use super::{Fold, ParameterAdmissionError};
use crate::types::{DataType, IntegerWidth};

impl Fold<'_, '_> {
    pub(super) fn validate_type_function_literal(
        &self,
        input: &str,
        target: &DataType,
    ) -> Result<(), ParameterAdmissionError> {
        use ScalarLiteralDiagnostic::{Range, Syntax};
        let diagnostic = if let Some(width) = IntegerWidth::from_data_type(target) {
            let name = match width {
                IntegerWidth::I16 => "smallint",
                IntegerWidth::I32 => "integer",
                IntegerWidth::I64 => "bigint",
            };
            crate::types::integer::parse(input, width)
                .err()
                .map(|error| match error {
                    crate::types::integer::IntegerInputError::Syntax => Syntax(name),
                    crate::types::integer::IntegerInputError::Range => Range(name),
                })
        } else {
            match target {
                DataType::Boolean => crate::types::boolean::parse_text(input)
                    .is_none()
                    .then_some(Syntax("boolean")),
                DataType::Uuid => crate::types::uuid_text::parse(input)
                    .is_none()
                    .then_some(Syntax("uuid")),
                DataType::Float => {
                    crate::types::float_input::parse(input)
                        .err()
                        .map(|error| match error {
                            crate::types::FloatInputError::Syntax => Syntax("double precision"),
                            crate::types::FloatInputError::Range => {
                                ScalarLiteralDiagnostic::FloatRange
                            }
                        })
                }
                _ => None,
            }
        };
        if let Some(diagnostic) = diagnostic {
            self.occurrences
                .invalid_scalar_literal(input, diagnostic, self.controls)?;
        }
        Ok(())
    }
}
