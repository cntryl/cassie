//! Preserve the rich array carrier when a cast has the same bound type.
use crate::sql::ast::Expr;
use crate::types::DataType;

pub(super) fn identity_cast_child(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Cast {
            expr,
            data_type: target @ DataType::Array(_),
            source_type: Some(source),
            ..
        } if source == target && !character_array(target) => Some(expr),
        _ => None,
    }
}

fn character_array(kind: &DataType) -> bool {
    match kind {
        DataType::Array(element) => {
            matches!(element.as_ref(), DataType::Char { length: Some(_) })
                || character_array(element)
        }
        _ => false,
    }
}
