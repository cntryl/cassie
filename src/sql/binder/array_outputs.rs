//! Polymorphic array results preserve the array identity rather than scalar inputs.

use crate::sql::functions::FunctionId;
use crate::types::DataType;

/// Ordinary function/operator result descriptors do not inherit argument typmods.
pub(crate) fn unmodified_result(kind: DataType) -> DataType {
    if !matches!(kind, DataType::Array(_)) {
        return kind;
    }
    i32::try_from(kind.type_oid())
        .ok()
        .and_then(super::parameter_data_type_for_oid)
        .unwrap_or(kind)
}

pub(crate) fn is_array_result(id: FunctionId) -> bool {
    matches!(
        id,
        FunctionId::ArrayCat
            | FunctionId::ArrayAppend
            | FunctionId::ArrayPrepend
            | FunctionId::ArrayFill
            | FunctionId::ArrayReverse
            | FunctionId::ArraySort
            | FunctionId::ArrayShuffle
            | FunctionId::ArraySample
            | FunctionId::TrimArray
    )
}

pub(crate) fn result_type(id: FunctionId, arguments: &[DataType]) -> Option<DataType> {
    let first = arguments.first()?;
    let array = |kind: &DataType| match kind {
        DataType::Array(element) => Some(element.as_ref().clone()),
        DataType::Null => Some(DataType::Null),
        _ => None,
    };
    let element = match id {
        FunctionId::ArrayCat => {
            super::inference::common_case_type(array(first)?, array(arguments.get(1)?)?)?
        }
        FunctionId::ArrayAppend => {
            super::inference::common_case_type(array(first)?, arguments.get(1)?.clone())?
        }
        FunctionId::ArrayPrepend => {
            super::inference::common_case_type(first.clone(), array(arguments.get(1)?)?)?
        }
        FunctionId::ArrayFill => first.clone(),
        FunctionId::ArrayReverse
        | FunctionId::ArraySort
        | FunctionId::ArrayShuffle
        | FunctionId::ArraySample
        | FunctionId::TrimArray => array(first)?,
        _ => return None,
    };
    Some(DataType::Array(Box::new(if element == DataType::Null {
        DataType::Text
    } else {
        element
    })))
}
