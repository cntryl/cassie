//! Reject selected math overloads PostgreSQL does not define.
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

fn is_numeric_argument(data_type: Option<DataType>) -> bool {
    matches!(
        data_type,
        Some(
            DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Extended(ExtendedType::Real | ExtendedType::Numeric(_))
        )
    )
}

fn is_unsupported_bucket_count(data_type: Option<DataType>) -> bool {
    matches!(
        data_type,
        Some(
            DataType::BigInt
                | DataType::Float
                | DataType::Extended(ExtendedType::Real | ExtendedType::Numeric(_))
        )
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
    if math == MathFunction::WidthBucket && function.args.len() == 4 {
        if types.declared_function(&function.name).is_none()
            && function.args[..3]
                .iter()
                .all(|argument| is_numeric_argument(types.expression_type(argument)))
            && is_unsupported_bucket_count(types.expression_type(&function.args[3]))
        {
            return Err(super::boolean_aggregates::undefined_function(
                function, types,
            ));
        }
        return Ok(());
    }
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
