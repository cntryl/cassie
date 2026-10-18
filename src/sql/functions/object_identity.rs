//! `pg_typeof` and the `to_reg*` catalog lookups.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
use crate::types::ObjectIdKind;

const fn lookup(id: FunctionId, name: &'static str, kind: ObjectIdKind) -> ScalarFunction {
    ScalarFunction {
        id,
        name,
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::ObjectId(kind),
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    lookup(FunctionId::PgTypeof, "pg_typeof", ObjectIdKind::RegType),
    lookup(
        FunctionId::ToRegClass,
        "to_regclass",
        ObjectIdKind::RegClass,
    ),
    lookup(
        FunctionId::ToRegCollation,
        "to_regcollation",
        ObjectIdKind::RegCollation,
    ),
    lookup(
        FunctionId::ToRegNamespace,
        "to_regnamespace",
        ObjectIdKind::RegNamespace,
    ),
    lookup(FunctionId::ToRegOper, "to_regoper", ObjectIdKind::RegOper),
    lookup(
        FunctionId::ToRegOperator,
        "to_regoperator",
        ObjectIdKind::RegOperator,
    ),
    lookup(FunctionId::ToRegProc, "to_regproc", ObjectIdKind::RegProc),
    lookup(
        FunctionId::ToRegProcedure,
        "to_regprocedure",
        ObjectIdKind::RegProcedure,
    ),
    lookup(FunctionId::ToRegRole, "to_regrole", ObjectIdKind::RegRole),
    lookup(FunctionId::ToRegType, "to_regtype", ObjectIdKind::RegType),
];

/// The `reg*` kind a `to_reg*` function looks up.
#[must_use]
pub(crate) fn lookup_kind(name: &str) -> Option<ObjectIdKind> {
    let local = name.strip_prefix("pg_catalog.").unwrap_or(name);
    FUNCTIONS
        .iter()
        .filter(|function| function.id != FunctionId::PgTypeof)
        .find(|function| function.name.eq_ignore_ascii_case(local))
        .and_then(|function| match function.return_type {
            FunctionReturnType::ObjectId(kind) => Some(kind),
            _ => None,
        })
}
