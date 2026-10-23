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
        FunctionId::Length,
        "char_length",
        FunctionArity::Exact(1),
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::Length,
        "character_length",
        FunctionArity::Exact(1),
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::Left,
        "left",
        FunctionArity::Exact(2),
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Right,
        "right",
        FunctionArity::Exact(2),
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::OctetLength,
        "octet_length",
        FunctionArity::Exact(1),
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::BitLength,
        "bit_length",
        FunctionArity::Exact(1),
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::Ascii,
        "ascii",
        FunctionArity::Exact(1),
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::Chr,
        "chr",
        FunctionArity::Exact(1),
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::StartsWith,
        "starts_with",
        FunctionArity::Exact(2),
        FunctionReturnType::Boolean,
    ),
    scalar(
        FunctionId::NumNulls,
        "num_nulls",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::NumNonNulls,
        "num_nonnulls",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Int,
    ),
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
    scalar(
        FunctionId::Replace,
        "replace",
        FunctionArity::Exact(3),
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Replace,
        "pg_catalog.replace",
        FunctionArity::Exact(3),
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Format,
        "format",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::Format,
        "pg_catalog.format",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Text,
    ),
];
