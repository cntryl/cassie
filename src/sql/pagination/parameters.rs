//! Concrete integer operands select a signature for unknown parameter siblings.
use crate::sql::ast::Expr;
use crate::types::DataType;

// The caller has already admitted the 128-level integer expression envelope.
pub(super) fn signature(expression: &Expr, oids: &[i32]) -> Option<DataType> {
    match expression {
        Expr::Param(index) => crate::sql::binder::parameter_data_type_for_oid(*oids.get(*index)?)
            .filter(super::is_integer),
        Expr::IntegerLiteral(value) => Some(crate::sql::binder::integer_literal_type(*value)),
        Expr::Cast { data_type, .. } if super::is_integer(data_type) => Some(data_type.clone()),
        Expr::IntegerNegate { width, .. } => Some(width.data_type()),
        Expr::UnaryNumeric { expr, .. } => signature(expr, oids),
        Expr::Binary { left, right, .. } => match (signature(left, oids), signature(right, oids)) {
            (Some(DataType::BigInt), _) | (_, Some(DataType::BigInt)) => Some(DataType::BigInt),
            (Some(DataType::Int), _) | (_, Some(DataType::Int)) => Some(DataType::Int),
            (Some(kind), _) | (_, Some(kind)) => Some(kind),
            _ => None,
        },
        _ => None,
    }
}
