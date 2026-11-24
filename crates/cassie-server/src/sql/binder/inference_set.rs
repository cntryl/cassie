//! Common set output types for integer widths and the selected exact numeric families.
use super::{DataType, Schema};
use crate::sql::ast::SetOperator;

pub(in crate::sql::binder) fn common_set_numeric_type(
    left: &DataType,
    right: &DataType,
) -> Option<DataType> {
    if matches!(left, DataType::SmallInt | DataType::Int | DataType::BigInt)
        && matches!(right, DataType::SmallInt | DataType::Int | DataType::BigInt)
    {
        return super::common_case_type(left.clone(), right.clone());
    }
    crate::types::extended::numeric_types::common(left, right)
}

pub(super) fn resolve_set_types(left: &mut Schema, right: &Schema, operator: SetOperator) {
    for (left, right) in left.fields.iter_mut().zip(&right.fields) {
        if let Some(common) = common_set_numeric_type(&left.data_type, &right.data_type) {
            left.data_type = common;
        } else if left.data_type == DataType::Null
            && matches!(operator, SetOperator::Union | SetOperator::UnionAll)
        {
            left.data_type = if right.data_type == DataType::Null {
                DataType::Text
            } else {
                right.data_type.clone()
            };
        }
    }
}
