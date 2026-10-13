//! Apply borrowed input signatures to already transformed argument occurrences.

use crate::app::CassieError;
use crate::sql::functions::FunctionInputSignature;
use crate::sql::{ast, functions};
use crate::types::DataType;

use super::check_controls;
use super::expressions::{common, oid};
use super::{Fold, Frame, ParameterAdmissionError, Summary};

impl Fold<'_, '_> {
    pub(super) fn function(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        let mut common_parent = false;
        for frame in self.frames.values.iter().rev() {
            check_controls(self.controls)?;
            match frame.node {
                super::Node::Expr(ast::Expr::Function(_)) => continue,
                super::Node::Function(function) => {
                    common_parent = functions::function(&function.name).is_some_and(|metadata| {
                        matches!(
                            functions::input_signature(metadata.id),
                            Some(FunctionInputSignature::CommonArguments)
                        )
                    });
                }
                _ => {}
            }
            break;
        }
        if function.args.is_empty() && !common_parent {
            return Ok(Summary::unqualified());
        }
        let occurrences = &mut self.occurrences;
        let summaries = &mut self.summaries.values[frame.start..];
        let declared = self
            .catalog
            .with_function_signature(
                &function.name,
                self.scope,
                Some(self.controls),
                |signature| {
                    let Some((arguments, result)) = signature else {
                        return Ok(None);
                    };
                    let mut qualified = arguments.len() == summaries.len();
                    for argument in arguments {
                        check_controls(self.controls)?;
                        qualified &= primitive(&argument.data_type);
                    }
                    if qualified {
                        for (summary, argument) in summaries.iter_mut().zip(arguments) {
                            check_controls(self.controls)?;
                            occurrences.coerce(summary, oid(&argument.data_type), true)?;
                        }
                    }
                    Ok(Some(if qualified && primitive(result) {
                        Summary::known(oid(result))
                    } else {
                        Summary::unqualified()
                    }))
                },
            )
            .map_err(admission_error)??;
        if let Some(summary) = declared {
            if !summary.qualified {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
            }
            return Ok(summary);
        }
        let metadata = functions::function(&function.name)
            .filter(|metadata| metadata.arity.matches(function.args.len()));
        let signature = metadata.and_then(|metadata| functions::input_signature(metadata.id));
        match signature {
            Some(FunctionInputSignature::Text) => {
                for summary in &mut self.summaries.values[frame.start..] {
                    check_controls(self.controls)?;
                    self.occurrences.coerce(summary, 25, true)?;
                }
                Ok(Summary::known(25))
            }
            Some(FunctionInputSignature::CommonArguments) => self.common_function(frame),
            None => {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
                Ok(Summary::unqualified())
            }
        }
    }

    pub(super) fn invalid_boolean_function_literals(
        &self,
        function: &ast::FunctionCall,
        summary: Summary,
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        if summary.oid != 16
            || !functions::function(&function.name).is_some_and(|metadata| {
                matches!(
                    functions::input_signature(metadata.id),
                    Some(FunctionInputSignature::CommonArguments)
                )
            })
        {
            return Ok(());
        }
        for (argument, argument_summary) in function
            .args
            .iter()
            .zip(&self.summaries.values[frame.start..])
        {
            check_controls(self.controls)?;
            if argument_summary.oid == 705 {
                if let ast::Expr::StringLiteral(input) = argument {
                    self.occurrences
                        .invalid_boolean_literal(input, self.controls)?;
                }
            }
        }
        Ok(())
    }

    fn common_function(&mut self, frame: &Frame<'_>) -> Result<Summary, ParameterAdmissionError> {
        let mut result = Summary::literal();
        for summary in &self.summaries.values[frame.start..] {
            check_controls(self.controls)?;
            if !summary.qualified {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
                return Ok(Summary::unqualified());
            }
            let Some(next) = common(result, *summary) else {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
                return Ok(Summary::unqualified());
            };
            result = Summary::known(next);
        }
        if result.oid == 705 || result.oid == 0 {
            result = Summary::known(25);
        }
        for summary in &mut self.summaries.values[frame.start..] {
            check_controls(self.controls)?;
            self.occurrences.coerce(summary, result.oid, true)?;
        }
        Ok(result)
    }
}

fn primitive(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Null
            | DataType::Boolean
            | DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Text
            | DataType::Char { .. }
            | DataType::Varchar { .. }
    )
}

fn admission_error(error: CassieError) -> ParameterAdmissionError {
    match error {
        CassieError::QueryCancelled => ParameterAdmissionError::Cancelled,
        CassieError::DeadlineExceeded => ParameterAdmissionError::Deadline,
        CassieError::ResourceLimit(message) => ParameterAdmissionError::Memory(message),
        other => ParameterAdmissionError::Memory(other.to_string()),
    }
}
