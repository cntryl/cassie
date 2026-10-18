//! Reject math overloads PostgreSQL does not define for `double precision`.
use super::super::coalesce_results::ResultTypes;
use crate::app::CassieError;
use crate::sql::ast::FunctionCall;
use crate::sql::functions::{FunctionId, MathFunction};
use crate::types::extended::ExtendedType;
use crate::types::DataType;

fn is_float(data_type: Option<&DataType>) -> bool {
    matches!(
        data_type,
        Some(DataType::Float | DataType::Extended(ExtendedType::Real))
    )
}

/// `round(double precision, integer)`, `trunc(double precision, integer)` and
/// `log(double precision, double precision)` do not exist (SQLSTATE 42883).
pub(super) fn validate(function: &FunctionCall, types: &ResultTypes) -> Result<(), CassieError> {
    let Some(metadata) = crate::sql::functions::function(&function.name) else {
        return Ok(());
    };
    let FunctionId::Math(math) = metadata.id else {
        return Ok(());
    };
    if function.args.len() != 2 {
        return Ok(());
    }
    let argument_types = function
        .args
        .iter()
        .map(|argument| types.expression_type(argument))
        .collect::<Vec<_>>();
    let rejected = match math {
        MathFunction::Round | MathFunction::Trunc => is_float(argument_types[0].as_ref()),
        MathFunction::Log => argument_types.iter().any(|kind| is_float(kind.as_ref())),
        _ => false,
    };
    if !rejected {
        return Ok(());
    }
    let names = argument_types
        .iter()
        .map(|kind| {
            kind.as_ref().map_or_else(
                || "unknown".to_owned(),
                super::boolean_aggregates::type_name,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    Err(CassieError::Execution(format!(
        "function {}({names}) does not exist",
        metadata.name
    )))
}
