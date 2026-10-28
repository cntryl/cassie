//! Qualify the selected document inspection signatures before scalar erasure.
use super::coalesce_results::ResultTypes;
use crate::app::CassieError;
use crate::sql::ast::{CastIdentity, Expr, FunctionCall};
use crate::sql::functions::{self, FunctionId, FunctionInputSignature};
use crate::types::DataType;

pub(super) fn qualify(function: &mut FunctionCall, types: &ResultTypes) -> Result<(), CassieError> {
    if types
        .declared_function(&function.name)
        .is_some_and(|metadata| metadata.args.len() == function.args.len())
    {
        return Ok(());
    }
    let Some(metadata) = functions::function(&function.name)
        .filter(|metadata| matches!(metadata.id, FunctionId::Json(_)))
    else {
        return Ok(());
    };
    let Some(
        signature @ (FunctionInputSignature::Positional(_)
        | FunctionInputSignature::VariadicTail { .. }),
    ) = functions::input_signature(metadata.id)
    else {
        return Ok(());
    };
    if !metadata.arity.matches(function.args.len())
        || function.args.iter().enumerate().any(|(index, argument)| {
            let Some(oid) = signature.argument_oid(index) else {
                return false;
            };
            !matches!(argument, Expr::StringLiteral(_) | Expr::Null)
                && types.expression_type(argument).is_some_and(|kind| {
                    kind != DataType::Null && Some(kind) != super::parameter_data_type_for_oid(oid)
                })
        })
    {
        return Err(undefined(function, types));
    }
    for (index, argument) in function.args.iter_mut().enumerate() {
        let Some(oid) = signature.argument_oid(index) else {
            break;
        };
        if matches!(argument, Expr::StringLiteral(_)) {
            let target = super::parameter_data_type_for_oid(oid).expect("registered JSON input");
            crate::sql::literal_inputs::validate_with_controls(
                argument,
                &target,
                CastIdentity::Scalar,
                types.controls(),
            )?;
            *argument = Expr::Cast {
                expr: Box::new(std::mem::replace(argument, Expr::Null)),
                data_type: target,
                source_type: None,
                identity: CastIdentity::Scalar,
            };
        }
    }
    Ok(())
}

fn undefined(function: &FunctionCall, types: &ResultTypes) -> CassieError {
    let arguments = function
        .args
        .iter()
        .map(|argument| {
            if matches!(argument, Expr::StringLiteral(_) | Expr::Null) {
                "unknown".to_owned()
            } else {
                types
                    .expression_type(argument)
                    .map_or_else(|| "unknown".to_owned(), |kind| kind.type_name())
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    CassieError::Execution(format!(
        "function {}({arguments}) does not exist",
        function.name
    ))
}
