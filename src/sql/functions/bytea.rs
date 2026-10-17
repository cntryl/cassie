//! Fixed BYTEA builtins and their qualified aliases.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
pub(crate) const fn is_bytea_function(id: FunctionId) -> bool {
    matches!(
        id,
        FunctionId::Encode
            | FunctionId::Decode
            | FunctionId::GetByte
            | FunctionId::SetByte
            | FunctionId::GetBit
            | FunctionId::SetBit
            | FunctionId::BitCount
            | FunctionId::Sha224
            | FunctionId::Sha256
            | FunctionId::Sha384
            | FunctionId::Sha512
    )
}
pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::Encode,
        name: "encode",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Encode,
        name: "pg_catalog.encode",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Decode,
        name: "decode",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Decode,
        name: "pg_catalog.decode",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GetByte,
        name: "get_byte",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GetByte,
        name: "pg_catalog.get_byte",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SetByte,
        name: "set_byte",
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SetByte,
        name: "pg_catalog.set_byte",
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GetBit,
        name: "get_bit",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GetBit,
        name: "pg_catalog.get_bit",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SetBit,
        name: "set_bit",
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SetBit,
        name: "pg_catalog.set_bit",
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BitCount,
        name: "bit_count",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::BigInt,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BitCount,
        name: "pg_catalog.bit_count",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::BigInt,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha224,
        name: "sha224",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha224,
        name: "pg_catalog.sha224",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha256,
        name: "sha256",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha256,
        name: "pg_catalog.sha256",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha384,
        name: "sha384",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha384,
        name: "pg_catalog.sha384",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha512,
        name: "sha512",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sha512,
        name: "pg_catalog.sha512",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Bytea,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
];

/// Select among the existing PostgreSQL BYTEA and pending BIT overloads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitInputSelection {
    Bytea,
    Ambiguous,
    PendingBit,
}

pub(crate) fn bit_input_selection(
    id: FunctionId,
    first_unknown: bool,
    index_oid: i32,
) -> BitInputSelection {
    if !first_unknown {
        return BitInputSelection::Bytea;
    }
    match id {
        FunctionId::BitCount => BitInputSelection::Ambiguous,
        FunctionId::GetBit | FunctionId::SetBit => match index_oid {
            20 => BitInputSelection::Bytea,
            21 | 23 => BitInputSelection::PendingBit,
            _ => BitInputSelection::Ambiguous,
        },
        _ => BitInputSelection::Bytea,
    }
}

/// A visible SQL routine with another arity cannot hide a matching fixed builtin.
pub(crate) fn has_distinct_builtin_arity(name: &str, actual: usize, declared: usize) -> bool {
    actual != declared
        && super::function(name).is_some_and(|metadata| {
            (is_bytea_function(metadata.id) || metadata.id == FunctionId::Lower)
                && metadata.arity.matches(actual)
        })
}
