//! Fixed BYTEA inputs are selected before unknown constants are coerced.
use super::{Fold, Frame, ParameterAdmissionError, Summary};
use crate::sql::{
    ast,
    functions::{self, FunctionInputSignature},
};
use crate::types::DataType;
impl Fold<'_, '_> {
    pub(super) fn fixed_bytea_function(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let Some(metadata) = functions::function(&function.name).filter(|value| {
            functions::is_bytea_function(value.id) || value.id == functions::FunctionId::Lower
        }) else {
            return Ok(None);
        };
        let expected = match functions::input_signature(metadata.id) {
            Some(FunctionInputSignature::Positional(oids)) => oids,
            Some(FunctionInputSignature::Text) => &[25],
            _ => return Ok(None),
        };
        let summaries = &self.summaries.values[frame.start..];
        if !metadata.arity.matches(function.args.len())
            || summaries.iter().zip(expected).any(|(summary, oid)| {
                summary.qualified
                    && !match oid {
                        17 => matches!(summary.oid, 0 | 17 | 705),
                        25 => matches!(summary.oid, 0 | 25 | 1042 | 1043 | 705),
                        20 => matches!(summary.oid, 0 | 20 | 21 | 23 | 705),
                        23 => {
                            matches!(summary.oid, 0 | 21 | 23 | 705)
                                || summary.oid == 20
                                    && self.occurrences.embedded_int4_carrier(*summary)
                        }
                        _ => false,
                    }
            })
        {
            self.occurrences
                .function_signature_error(&function.name, summaries, self.controls)?;
            return Ok(Some(Summary::unqualified()));
        }
        let selection = functions::bit_input_selection(
            metadata.id,
            summaries
                .first()
                .is_some_and(|summary| matches!(summary.oid, 0 | 705)),
            summaries.get(1).map_or(705, |summary| summary.oid),
        );
        match selection {
            functions::BitInputSelection::Bytea => {}
            functions::BitInputSelection::Ambiguous => {
                self.occurrences.resolution_error(
                    &function.name,
                    summaries,
                    "42725",
                    ") is not unique",
                    self.controls,
                )?;
                return Ok(Some(Summary::unqualified()));
            }
            functions::BitInputSelection::PendingBit => {
                if self.occurrences.stops_on_error() {
                    return Err(super::super::function_diagnostics::admit(
                        128,
                        "0A000",
                        self.controls,
                        |message| {
                            message.push_str("BIT overload input remains pending under #793/#816; use an explicitly typed BYTEA argument");
                        },
                    )?);
                }
                return Ok(Some(Summary::unqualified()));
            }
        }
        for (index, (argument, oid)) in function.args.iter().zip(expected).enumerate() {
            super::check_controls(self.controls)?;
            if let ast::Expr::StringLiteral(input) = argument {
                match oid {
                    17 => self.occurrences.bytea_literal(input, self.controls)?,
                    20 => self.validate_type_function_literal(input, &DataType::BigInt)?,
                    23 => self.validate_type_function_literal(input, &DataType::Int)?,
                    _ => {}
                }
            }
            self.occurrences
                .coerce(&mut self.summaries.values[frame.start + index], *oid, true)?;
        }
        Ok(Some(Summary::known(match metadata.return_type {
            functions::FunctionReturnType::Bytea => 17,
            functions::FunctionReturnType::Int => 23,
            functions::FunctionReturnType::BigInt => 20,
            _ => 25,
        })))
    }
}

#[cfg(test)]
#[path = "bytea_function_tests.rs"]
mod tests;
