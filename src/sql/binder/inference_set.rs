//! Result type resolution for unknown NULL outputs in UNION branches.
use super::{DataType, Schema};
use crate::sql::ast::SetOperator;

pub(super) fn inherit_unknown_union_types(
    left: &mut Schema,
    right: &Schema,
    operator: SetOperator,
) {
    if !matches!(operator, SetOperator::Union | SetOperator::UnionAll) {
        return;
    }
    for (left, right) in left.fields.iter_mut().zip(&right.fields) {
        if left.data_type == DataType::Null {
            left.data_type = if right.data_type == DataType::Null {
                DataType::Text
            } else {
                right.data_type.clone()
            };
        }
    }
}
