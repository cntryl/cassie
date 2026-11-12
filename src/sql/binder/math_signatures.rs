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

fn is_integer_scale(data_type: Option<&DataType>) -> bool {
    data_type.is_none_or(|data_type| {
        matches!(
            data_type,
            DataType::SmallInt | DataType::Int | DataType::Null
        )
    })
}

fn is_numeric_argument(data_type: Option<&DataType>) -> bool {
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

fn is_unsupported_bucket_count(data_type: Option<&DataType>) -> bool {
    matches!(
        data_type,
        Some(
            DataType::BigInt
                | DataType::Float
                | DataType::Extended(ExtendedType::Real | ExtendedType::Numeric(_))
        )
    )
}

fn bucket_literal_family(data_type: Option<&DataType>) -> Option<bool> {
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

fn validate_width_bucket(
    function: &mut FunctionCall,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if !has_applicable_declared_routine(function, types) {
        let first = bucket_literal_family(types.expression_type(&function.args[0]).as_ref());
        if first.is_some()
            && function.args[1..3].iter().all(|argument| {
                bucket_literal_family(types.expression_type(argument).as_ref()) == first
            })
        {
            coerce_int4_literal(function, 3, types)?;
        }
    }
    if !has_applicable_declared_routine(function, types)
        && function.args[..3]
            .iter()
            .all(|argument| is_numeric_argument(types.expression_type(argument).as_ref()))
        && is_unsupported_bucket_count(types.expression_type(&function.args[3]).as_ref())
    {
        return Err(super::boolean_aggregates::undefined_function(
            function, types,
        ));
    }
    Ok(())
}

/// Rewrites a string literal argument to an INT4 literal, reporting the
/// PostgreSQL INT4 input error when it does not parse.
fn coerce_int4_literal(
    function: &mut FunctionCall,
    index: usize,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    let Expr::StringLiteral(input) = &function.args[index] else {
        return Ok(());
    };
    if let Some(controls) = types.controls() {
        controls.check_interruption()?;
    }
    let Ok(value) = crate::types::integer::parse(input, crate::types::IntegerWidth::I32) else {
        crate::sql::literal_inputs::validate_with_controls(
            &function.args[index],
            &DataType::Int,
            CastIdentity::Scalar,
            types.controls(),
        )?;
        unreachable!("INT4 input validation uses the same parser");
    };
    function.args[index] = Expr::IntegerLiteral(value);
    Ok(())
}

/// Selected PostgreSQL math overloads do not exist (SQLSTATE 42883):
/// `round`/`trunc` with FLOAT8 or a non-INT4 scale, two-argument FLOAT8 `log`,
/// and `mod` over REAL/FLOAT8.
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
        return validate_width_bucket(function, types);
    }
    if matches!(math, MathFunction::Round | MathFunction::Trunc)
        && function.args.len() == 2
        && !has_applicable_declared_routine(function, types)
    {
        coerce_int4_literal(function, 1, types)?;
    }
    if function.args.len() != 2 {
        return Ok(());
    }
    let argument_types = function
        .args
        .iter()
        .map(|argument| types.expression_type(argument))
        .collect::<Vec<_>>();
    let has_declared_routine = has_applicable_declared_routine(function, types);
    if math == MathFunction::Mod
        && !has_declared_routine
        && argument_types.iter().any(|kind| is_float(kind.as_ref()))
    {
        return Err(super::boolean_aggregates::undefined_function(
            function, types,
        ));
    }
    let rejected = match math {
        MathFunction::Round | MathFunction::Trunc => {
            !has_declared_routine
                && (is_float(argument_types[0].as_ref())
                    || !is_integer_scale(argument_types[1].as_ref()))
        }
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
