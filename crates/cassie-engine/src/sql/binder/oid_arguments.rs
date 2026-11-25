//! Bind the PostgreSQL implicit inputs to oid and regclass catalog functions.
use super::{
    coalesce_results::ResultTypes, source_bindings::cast_input, CassieError, DataType, Expr,
};
use crate::types::ObjectIdKind;

/// Unknown inputs acquire the declared parameter type. Already typed inputs
/// must have an implicit cast to the target, independent of the value carrier.
pub(super) fn coerce(
    name: &str,
    argument: &mut Expr,
    target: ObjectIdKind,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if !matches!(argument, Expr::StringLiteral(_) | Expr::Null) {
        if let Some(source) = types.expression_type(argument) {
            if !accepts(&source, target) {
                return Err(crate::app::DiagnosticError::new(
                    "42883",
                    format!("function {name}({}) does not exist", source.type_name()),
                )
                .into());
            }
        }
    }
    cast_input(argument, &DataType::ObjectId(target), types);
    Ok(())
}

fn accepts(source: &DataType, target: ObjectIdKind) -> bool {
    match source {
        DataType::Domain(domain) => accepts(&domain.base, target),
        DataType::Null | DataType::SmallInt | DataType::Int | DataType::BigInt => true,
        DataType::Text | DataType::Varchar { .. } => target == ObjectIdKind::RegClass,
        DataType::ObjectId(kind) => {
            target == ObjectIdKind::Oid || *kind == target || *kind == ObjectIdKind::Oid
        }
        _ => false,
    }
}
