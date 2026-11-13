//! Named enum support functions (#827): `enum_first`, `enum_last`, `enum_range`.
//!
//! Their argument is polymorphic (`anyenum`): the result follows the argument's
//! enum type, so the first non-NULL argument type is the result type and
//! `enum_range` wraps it in an array.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::EnumFirst,
        name: "enum_first",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::EnumLast,
        name: "enum_last",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::EnumRange,
        name: "enum_range",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
];
