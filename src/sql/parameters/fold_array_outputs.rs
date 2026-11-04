//! Array-producing expressions export native array summaries to surrounding sources.
use super::{Fold, Frame, ParameterAdmissionError, Summary};
use crate::sql::{ast, binder, functions};
use crate::types::DataType;

impl Fold<'_, '_> {
    pub(super) fn array_function_output(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let constructor = function
            .name
            .eq_ignore_ascii_case(ast::ARRAY_CONSTRUCTOR_FUNCTION);
        let metadata = functions::function(&function.name)
            .filter(|metadata| binder::array_outputs::is_array_result(metadata.id));
        if !constructor && metadata.is_none() {
            return Ok(None);
        }
        let args = &mut self.summaries.values[frame.start..frame.start + function.args.len()];
        let types = args
            .iter()
            .map(|summary| {
                binder::parameter_data_type_for_oid(summary.oid).unwrap_or(DataType::Null)
            })
            .collect::<Vec<_>>();
        let output = if constructor {
            let mut element = DataType::Null;
            for kind in types {
                let left = i32::try_from(element.type_oid()).unwrap_or(705);
                let right = i32::try_from(kind.type_oid()).unwrap_or(705);
                let Some(common) = binder::common_case_type(element, kind) else {
                    self.occurrences.array_types(left, right)?;
                    return Ok(Some(Summary::unqualified()));
                };
                element = common;
            }
            if element == DataType::Null {
                element = DataType::Text;
            }
            let input = i32::try_from(element.type_oid()).unwrap_or(705);
            for argument in args {
                if matches!(argument.oid, 0 | 705) {
                    self.occurrences.coerce(argument, input, false)?;
                }
            }
            while let DataType::Array(inner) = element {
                element = *inner;
            }
            DataType::Array(Box::new(element))
        } else {
            let metadata = metadata.expect("array producer metadata");
            if !metadata.arity.matches(function.args.len()) {
                return Ok(None);
            }
            if metadata.id == functions::FunctionId::ArrayFill
                && types.first() == Some(&DataType::Null)
            {
                self.occurrences.polymorphic_unknown_input()?;
                return Ok(Some(Summary::unqualified()));
            }
            let Some(output) = binder::array_outputs::result_type(metadata.id, &types) else {
                return Ok(None);
            };
            let DataType::Array(element) = &output else {
                unreachable!("array result policy");
            };
            let array_oid = i32::try_from(output.type_oid()).unwrap_or(705);
            let element_oid = i32::try_from(element.type_oid()).unwrap_or(705);
            for (index, argument) in args.iter_mut().enumerate() {
                let target = match metadata.id {
                    functions::FunctionId::ArrayPrepend if index == 0 => element_oid,
                    functions::FunctionId::ArrayAppend if index == 1 => element_oid,
                    functions::FunctionId::ArrayFill if index == 0 => element_oid,
                    functions::FunctionId::ArrayFill => 1_007,
                    functions::FunctionId::ArraySample | functions::FunctionId::TrimArray
                        if index == 1 =>
                    {
                        23
                    }
                    functions::FunctionId::ArraySort if index > 0 => 16,
                    _ => array_oid,
                };
                if matches!(argument.oid, 0 | 705) {
                    self.occurrences.coerce(argument, target, false)?;
                }
            }
            output
        };
        Ok(Some(Summary::known(
            i32::try_from(output.type_oid()).unwrap_or(705),
        )))
    }
}
