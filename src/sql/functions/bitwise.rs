//! Integer bitwise aggregate identities.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    aggregate(FunctionId::BitAnd, "bit_and"),
    aggregate(FunctionId::BitOr, "bit_or"),
    aggregate(FunctionId::BitXor, "bit_xor"),
];

const fn aggregate(id: FunctionId, name: &'static str) -> ScalarFunction {
    ScalarFunction {
        id,
        name,
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::FirstComparedArgument,
        is_aggregate: true,
        bypass_arity_validation: false,
    }
}
