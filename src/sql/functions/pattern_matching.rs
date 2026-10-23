//! Registered regular-expression scalar functions with stable SQL identities.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

const fn scalar(
    id: FunctionId,
    name: &'static str,
    arity: FunctionArity,
    return_type: FunctionReturnType,
) -> ScalarFunction {
    ScalarFunction {
        id,
        name,
        arity,
        return_type,
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    scalar(
        FunctionId::RegexpLike,
        "regexp_like",
        FunctionArity::Range { min: 2, max: 3 },
        FunctionReturnType::Boolean,
    ),
    scalar(
        FunctionId::RegexpLike,
        "pg_catalog.regexp_like",
        FunctionArity::Range { min: 2, max: 3 },
        FunctionReturnType::Boolean,
    ),
    scalar(
        FunctionId::RegexpCount,
        "regexp_count",
        FunctionArity::Range { min: 2, max: 4 },
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::RegexpCount,
        "pg_catalog.regexp_count",
        FunctionArity::Range { min: 2, max: 4 },
        FunctionReturnType::Int,
    ),
];
