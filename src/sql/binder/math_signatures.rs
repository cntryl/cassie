//! Reject selected math overloads PostgreSQL does not define.
use super::super::coalesce_results::ResultTypes;
use crate::app::CassieError;
use crate::sql::ast::{CastIdentity, Expr, FunctionCall};
use crate::sql::functions::{self, FunctionId, MathFunction};
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

fn bucket_literal_family(data_type: Option<DataType>) -> Option<bool> {
    match data_type {
        Some(DataType::Float) => Some(false),
        Some(DataType::Extended(ExtendedType::Numeric(_))) => Some(true),
        _ => None,
    }
}

fn has_applicable_declared_routine(function: &FunctionCall, types: &ResultTypes) -> bool {
    types
        .declared_function(&function.name)
        .is_some_and(|metadata| {
            !functions::has_distinct_builtin_arity(
                &function.name,
                function.args.len(),
                metadata.args.len(),
            )
        })
}

/// `round(double precision, integer)`, `trunc(double precision, integer)` and
/// `log(double precision, double precision)` do not exist (SQLSTATE 42883).
pub(super) fn validate(
    function: &mut FunctionCall,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    let Some(metadata) = crate::sql::functions::function(&function.name) else {
        return Ok(());
    };
    let FunctionId::Math(math) = metadata.id else {
        return Ok(());
    };
    if math == MathFunction::WidthBucket && function.args.len() == 4 {
        if !has_applicable_declared_routine(function, types) {
            let first = bucket_literal_family(types.expression_type(&function.args[0]));
            if first.is_some()
                && function.args[1..3]
                    .iter()
                    .all(|argument| bucket_literal_family(types.expression_type(argument)) == first)
            {
                if let Expr::StringLiteral(input) = &function.args[3] {
                    if let Some(controls) = types.controls() {
                        controls.check_interruption()?;
                    }
                    let value = match crate::types::integer::parse(
                        input,
                        crate::types::IntegerWidth::I32,
                    ) {
                        Ok(value) => value,
                        Err(_) => {
                            crate::sql::literal_inputs::validate_with_controls(
                                &function.args[3],
                                &DataType::Int,
                                CastIdentity::Scalar,
                                types.controls(),
                            )?;
                            unreachable!("INT4 input validation uses the same parser");
                        }
                    };
                    function.args[3] = Expr::IntegerLiteral(value);
                }
            }
        }
        if !has_applicable_declared_routine(function, types)
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
