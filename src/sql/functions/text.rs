//! Metadata shared by text selection aliases and their wire descriptors.

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
        FunctionId::Substring,
        "substr",
        FunctionArity::Range { min: 2, max: 3 },
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Trim,
        "btrim",
        FunctionArity::Range { min: 1, max: 2 },
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Ltrim,
        "ltrim",
        FunctionArity::Range { min: 1, max: 2 },
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Rtrim,
        "rtrim",
        FunctionArity::Range { min: 1, max: 2 },
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::SplitPart,
        "split_part",
        FunctionArity::Exact(3),
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Strpos,
        "strpos",
        FunctionArity::Exact(2),
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::Strpos,
        "position",
        FunctionArity::Exact(2),
        FunctionReturnType::Int,
    ),
];
