//! Narrow, source-proven missing `to_char(TIMETZ, unknown)` signatures.

use crate::sql::ast::{Expr, FunctionCall};
use crate::types::extended::ExtendedType;
use crate::types::DataType;

/// Returns the qualification when this exact argument shape is selected.
pub(crate) fn candidate(
    function: &FunctionCall,
    original_wire_timetz: impl FnOnce(usize) -> bool,
) -> Option<bool> {
    if function.has_modifiers() {
        return None;
    }
    let qualified = if function.name.eq_ignore_ascii_case("to_char") {
        false
    } else if function.name.eq_ignore_ascii_case("pg_catalog.to_char") {
        true
    } else {
        return None;
    };
    let [first, second] = function.args.as_slice() else {
        return None;
    };
    if !matches!(second, Expr::Null | Expr::StringLiteral(_)) {
        return None;
    }
    let selected = match first {
        Expr::Cast {
            expr,
            data_type: DataType::Extended(ExtendedType::TimeTz { .. }),
            ..
        } => {
            matches!(expr.as_ref(), Expr::Null)
        }
        Expr::Param(slot) => original_wire_timetz(*slot),
        _ => false,
    };
    selected.then_some(qualified)
}
