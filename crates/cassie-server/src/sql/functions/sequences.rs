//! Sequence manipulation functions. They are volatile: never folded or cached.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

/// Every spelling the result cache treats as sequence-mutating or session-dependent.
pub(crate) const SEQUENCE_FUNCTION_NAMES: [&str; 8] = [
    "nextval",
    "currval",
    "setval",
    "lastval",
    "pg_catalog.nextval",
    "pg_catalog.currval",
    "pg_catalog.setval",
    "pg_catalog.lastval",
];

const fn sequence_function(
    id: FunctionId,
    name: &'static str,
    arity: FunctionArity,
) -> ScalarFunction {
    ScalarFunction {
        id,
        name,
        arity,
        return_type: FunctionReturnType::BigInt,
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    sequence_function(FunctionId::Nextval, "nextval", FunctionArity::Exact(1)),
    sequence_function(FunctionId::Currval, "currval", FunctionArity::Exact(1)),
    sequence_function(
        FunctionId::Setval,
        "setval",
        FunctionArity::Range { min: 2, max: 3 },
    ),
    sequence_function(FunctionId::Lastval, "lastval", FunctionArity::Exact(0)),
    ScalarFunction {
        id: FunctionId::PgGetSerialSequence,
        name: "pg_get_serial_sequence",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
];
