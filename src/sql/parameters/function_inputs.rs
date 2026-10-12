//! Input inference for the qualified Lower/COALESCE and primitive SQL-function slice.
//!
//! Catalog signatures are borrowed, never copied with their stored bodies. Unknown
//! finalization is local to an eligible COALESCE, not a statement-wide fallback.

use super::inference::{infer_parameter_type_from_expected_expr, infer_parameter_type_oids_expr};
use super::{ParameterAdmissionError, ParameterInference, UNKNOWN_PARAMETER_TYPE_OID};
use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::sql::ast::{Expr, FunctionCall};
use crate::sql::functions::{self, FunctionInputSignature, FunctionReturnType};
use crate::sql::{binder, FieldTypeMap};
use crate::types::DataType;

pub(super) fn infer_function_parameter_type_oids(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) {
    if !inference.check() || function.args.is_empty() {
        return;
    }
    for argument in &function.args {
        infer_parameter_type_oids_expr(argument, field_types, catalog, inference);
    }
    if !inference.check() {
        return;
    }
    if let Err(error) = infer_inputs(function, field_types, catalog, inference) {
        inference.failure = Some(error);
    }
}

fn infer_inputs(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<(), ParameterAdmissionError> {
    let declared = catalog
        .with_function_signature(
            &function.name,
            inference.scope,
            inference.controls,
            |signature| {
                let Some((arguments, _)) = signature else {
                    return false;
                };
                if arguments.len() == function.args.len() {
                    for (argument, input) in function.args.iter().zip(arguments) {
                        if let Some(data_type) = primitive_type(&input.data_type) {
                            // This visitor only sees Params/Casts; it cannot reenter
                            // the catalog while the signature read guard is held.
                            infer_parameter_type_from_expected_expr(
                                argument, &data_type, inference,
                            );
                        }
                    }
                }
                true
            },
        )
        .map_err(admission_error)?;
    if declared || !inference.check() {
        return Ok(());
    }
    let Some(metadata) = functions::function(&function.name)
        .filter(|metadata| metadata.arity.matches(function.args.len()))
    else {
        return Ok(());
    };
    let data_type = match functions::input_signature(metadata.id) {
        Some(FunctionInputSignature::Text) => Some(DataType::Text),
        Some(FunctionInputSignature::CommonArguments) => {
            common_argument_type(function, field_types, catalog, inference)?
        }
        None => None,
    };
    if let Some(data_type) = data_type {
        for argument in &function.args {
            infer_parameter_type_from_expected_expr(argument, &data_type, inference);
        }
    }
    Ok(())
}

enum ArgumentType {
    Known(DataType),
    Unknown,
    Unqualified,
}

fn common_argument_type(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<Option<DataType>, ParameterAdmissionError> {
    let mut common = DataType::Null;
    for argument in &function.args {
        if !inference.check() {
            return Ok(None);
        }
        let data_type = match argument_type(argument, field_types, catalog, inference)? {
            ArgumentType::Known(data_type) => data_type,
            ArgumentType::Unknown => continue,
            ArgumentType::Unqualified => return Ok(None),
        };
        let Some(next) = binder::common_case_type(common, data_type) else {
            return Ok(None);
        };
        common = next;
    }
    Ok(Some(if common == DataType::Null {
        DataType::Text
    } else {
        common
    }))
}

fn argument_type(
    argument: &Expr,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<ArgumentType, ParameterAdmissionError> {
    if !inference.check() {
        return Ok(ArgumentType::Unqualified);
    }
    let known = match argument {
        Expr::Param(index) => {
            let oid = inference
                .oids
                .get(*index)
                .copied()
                .unwrap_or(UNKNOWN_PARAMETER_TYPE_OID);
            if matches!(oid, 0 | UNKNOWN_PARAMETER_TYPE_OID) {
                return Ok(ArgumentType::Unknown);
            }
            u32::try_from(oid)
                .ok()
                .and_then(crate::types::builtin::builtin_for_oid)
                .and_then(|metadata| primitive_type(&metadata.kind.data_type()))
        }
        Expr::Null => return Ok(ArgumentType::Unknown),
        Expr::Cast { data_type, .. } => primitive_type(data_type),
        Expr::Column(name) => {
            crate::sql::field_type_for_column(field_types, name).and_then(primitive_type)
        }
        Expr::Function(function) => {
            return function_result_type(function, field_types, catalog, inference);
        }
        _ => binder::known_expr_type(argument, field_types)
            .and_then(|data_type| primitive_type(&data_type)),
    };
    Ok(known.map_or(ArgumentType::Unqualified, ArgumentType::Known))
}

fn function_result_type(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<ArgumentType, ParameterAdmissionError> {
    let declared = catalog
        .with_function_signature(
            &function.name,
            inference.scope,
            inference.controls,
            |signature| signature.map(|(_, result)| primitive_type(result)),
        )
        .map_err(admission_error)?;
    if let Some(data_type) = declared {
        return Ok(data_type.map_or(ArgumentType::Unqualified, ArgumentType::Known));
    }
    let Some(metadata) = functions::function(&function.name)
        .filter(|metadata| metadata.arity.matches(function.args.len()))
    else {
        return Ok(ArgumentType::Unqualified);
    };
    match functions::input_signature(metadata.id) {
        Some(FunctionInputSignature::CommonArguments) => {
            Ok(
                common_argument_type(function, field_types, catalog, inference)?
                    .map_or(ArgumentType::Unqualified, ArgumentType::Known),
            )
        }
        Some(FunctionInputSignature::Text) if metadata.return_type == FunctionReturnType::Text => {
            Ok(ArgumentType::Known(DataType::Text))
        }
        _ => Ok(ArgumentType::Unqualified),
    }
}

fn primitive_type(data_type: &DataType) -> Option<DataType> {
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
    .then(|| data_type.clone())
}

fn admission_error(error: CassieError) -> ParameterAdmissionError {
    match error {
        CassieError::QueryCancelled => ParameterAdmissionError::Cancelled,
        CassieError::DeadlineExceeded => ParameterAdmissionError::Deadline,
        CassieError::ResourceLimit(message) => ParameterAdmissionError::Memory(message),
        other => ParameterAdmissionError::Memory(other.to_string()),
    }
}
