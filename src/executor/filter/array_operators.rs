//! SQL array containment, overlap, and concatenation operators.
use super::{BinaryOp, QueryError, ScalarValue};
use crate::types::extended::TypedOperator;
use crate::types::{ArrayDimension, SqlArray, Value};

pub(super) fn concatenate_operand(
    left: &ScalarValue,
    right: &ScalarValue,
    left_null_type: (bool, bool),
    right_null_type: (bool, bool),
) -> Option<Result<ScalarValue, QueryError>> {
    let (left_typed_array_null, left_typed_scalar_null) = left_null_type;
    let (right_typed_array_null, right_typed_scalar_null) = right_null_type;
    let is_array = matches!(left, ScalarValue::Array(_))
        || matches!(right, ScalarValue::Array(_))
        || left_typed_array_null
        || right_typed_array_null;
    if !is_array {
        return None;
    }
    let result = match (left, right) {
        (ScalarValue::Array(array), ScalarValue::Array(other)) => {
            concatenate(array, other).map(ScalarValue::Array)
        }
        (ScalarValue::Array(array), ScalarValue::Null) if right_typed_scalar_null => {
            extend_scalar(array, Value::Null, false).map(ScalarValue::Array)
        }
        (ScalarValue::Null, ScalarValue::Null)
            if left_typed_array_null && right_typed_scalar_null =>
        {
            one_element(Value::Null).map(ScalarValue::Array)
        }
        (ScalarValue::Null, ScalarValue::Array(array)) if left_typed_scalar_null => {
            extend_scalar(array, Value::Null, true).map(ScalarValue::Array)
        }
        (ScalarValue::Array(array), ScalarValue::Null)
        | (ScalarValue::Null, ScalarValue::Array(array)) => Ok(ScalarValue::Array(array.clone())),
        (ScalarValue::Array(array), ScalarValue::Str(text)) => parse_array_text(text, array)
            .and_then(|other| concatenate(array, &other))
            .map(ScalarValue::Array),
        (ScalarValue::Str(text), ScalarValue::Array(array)) => parse_array_text(text, array)
            .and_then(|other| concatenate(&other, array))
            .map(ScalarValue::Array),
        (ScalarValue::Null, ScalarValue::Null) if left_typed_array_null => Ok(ScalarValue::Null),
        (ScalarValue::Null, ScalarValue::Null) if right_typed_array_null => Ok(ScalarValue::Null),
        (ScalarValue::Null, scalar) if left_typed_array_null => {
            one_element(scalar.to_value()).map(ScalarValue::Array)
        }
        (scalar, ScalarValue::Null) if right_typed_array_null => {
            one_element(scalar.to_value()).map(ScalarValue::Array)
        }
        (ScalarValue::Array(array), scalar) => {
            extend_scalar(array, scalar.to_value(), false).map(ScalarValue::Array)
        }
        (scalar, ScalarValue::Array(array)) => {
            extend_scalar(array, scalar.to_value(), true).map(ScalarValue::Array)
        }
        _ => return None,
    };
    Some(result)
}

fn one_element(value: Value) -> Result<SqlArray, QueryError> {
    SqlArray::from_elements(vec![value])
        .map_err(|error| QueryError::Cassie(crate::app::CassieError::ArrayInput(error)))
}

fn parse_array_text(text: &str, array: &SqlArray) -> Result<SqlArray, QueryError> {
    let element_type = array
        .elements()
        .iter()
        .find(|value| !matches!(value, Value::Null))
        .map_or(crate::types::DataType::Text, element_type);
    let parsed = crate::types::array::parse_text_array(text, &element_type).map_err(|error| {
        QueryError::Cassie(crate::app::CassieError::ArrayInput(error.to_string()))
    })?;
    SqlArray::from_nested_json(&parsed, element_type.is_json())
        .map_err(|error| QueryError::Cassie(crate::app::CassieError::ArrayInput(error)))
}

fn element_type(value: &Value) -> crate::types::DataType {
    match value {
        Value::Bool(_) => crate::types::DataType::Boolean,
        Value::Int64(value) if i32::try_from(*value).is_ok() => crate::types::DataType::Int,
        Value::Int64(_) => crate::types::DataType::BigInt,
        Value::Float64(_) => crate::types::DataType::Float,
        Value::Json(_) => crate::types::DataType::Jsonb,
        Value::Typed(value) => crate::types::DataType::Extended(value.kind()),
        Value::Null | Value::String(_) | Value::Vector(_) | Value::Array(_) => {
            crate::types::DataType::Text
        }
    }
}

pub(super) fn evaluate(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
) -> Option<Result<ScalarValue, QueryError>> {
    if matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IsDistinctFrom
            | BinaryOp::IsNotDistinctFrom
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
    ) {
        let is_array =
            matches!(left, ScalarValue::Array(_)) || matches!(right, ScalarValue::Array(_));
        if is_array {
            if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
                if matches!(op, BinaryOp::IsDistinctFrom | BinaryOp::IsNotDistinctFrom) {
                    let equal =
                        matches!(left, ScalarValue::Null) && matches!(right, ScalarValue::Null);
                    return Some(Ok(ScalarValue::Bool(match op {
                        BinaryOp::IsDistinctFrom => !equal,
                        BinaryOp::IsNotDistinctFrom => equal,
                        _ => unreachable!("only null-safe comparisons reach this branch"),
                    })));
                }
                return Some(Ok(ScalarValue::Null));
            }
            let (ScalarValue::Array(left), ScalarValue::Array(right)) = (left, right) else {
                return Some(Err(unsupported(op)));
            };
            return Some(compare_result(left, op, right));
        }
    }
    let operator = match op {
        BinaryOp::Concat => Some(TypedOperator::Concat),
        _ => op.typed_operator(),
    }?;
    let is_array = matches!(left, ScalarValue::Array(_)) || matches!(right, ScalarValue::Array(_));
    if !is_array {
        return None;
    }
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Some(Ok(ScalarValue::Null));
    }
    let (ScalarValue::Array(left), ScalarValue::Array(right)) = (left, right) else {
        return Some(Err(unsupported(op)));
    };
    let result = match operator {
        TypedOperator::ContainsOrEqual => Ok(ScalarValue::Bool(contains(left, right))),
        TypedOperator::ContainedOrEqual => Ok(ScalarValue::Bool(contains(right, left))),
        TypedOperator::Overlap => Ok(ScalarValue::Bool(overlaps(left, right))),
        TypedOperator::Concat => concatenate(left, right).map(ScalarValue::Array),
        _ => return None,
    };
    Some(result)
}

fn compare_result(
    left: &SqlArray,
    op: &BinaryOp,
    right: &SqlArray,
) -> Result<ScalarValue, QueryError> {
    let ordering = compare_arrays(left, right).ok_or_else(|| unsupported(op))?;
    let result = match op {
        BinaryOp::Eq | BinaryOp::IsNotDistinctFrom => ordering.is_eq(),
        BinaryOp::NotEq | BinaryOp::IsDistinctFrom => ordering.is_ne(),
        BinaryOp::Lt => ordering.is_lt(),
        BinaryOp::Lte => ordering.is_le(),
        BinaryOp::Gt => ordering.is_gt(),
        BinaryOp::Gte => ordering.is_ge(),
        _ => unreachable!("only array comparison operators reach this function"),
    };
    Ok(ScalarValue::Bool(result))
}

pub(crate) fn compare_arrays(left: &SqlArray, right: &SqlArray) -> Option<std::cmp::Ordering> {
    for (left_element, right_element) in left.elements().iter().zip(right.elements()) {
        let ordering = compare_elements(left_element, right_element)?;
        if !ordering.is_eq() {
            return Some(ordering);
        }
    }
    let ordering = left.len().cmp(&right.len());
    if !ordering.is_eq() {
        return Some(ordering);
    }
    let ordering = left.dimensions().len().cmp(&right.dimensions().len());
    if !ordering.is_eq() {
        return Some(ordering);
    }
    for (left, right) in left.dimensions().iter().zip(right.dimensions()) {
        let ordering = left.length.cmp(&right.length);
        if !ordering.is_eq() {
            return Some(ordering);
        }
    }
    for (left, right) in left.dimensions().iter().zip(right.dimensions()) {
        let ordering = left.lower_bound.cmp(&right.lower_bound);
        if !ordering.is_eq() {
            return Some(ordering);
        }
    }
    Some(std::cmp::Ordering::Equal)
}

fn compare_elements(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
        (Value::Null, _) => Some(std::cmp::Ordering::Greater),
        (_, Value::Null) => Some(std::cmp::Ordering::Less),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Typed(left), Value::Typed(right)) => {
            crate::types::extended::operations::compare(left, right)
        }
        (Value::Vector(left), Value::Vector(right)) => Some(
            left.values
                .iter()
                .zip(&right.values)
                .map(|(left, right)| left.total_cmp(right))
                .find(|ordering| !ordering.is_eq())
                .unwrap_or_else(|| left.values.len().cmp(&right.values.len())),
        ),
        (Value::Json(left), Value::Json(right)) => {
            super::json_order::compare(&left.to_string(), &right.to_string())
        }
        (Value::Int64(_) | Value::Float64(_), Value::Int64(_) | Value::Float64(_)) => {
            crate::executor::semantic::compare_numeric_values(left, right)
        }
        _ => None,
    }
}

fn contains(candidate: &SqlArray, required: &SqlArray) -> bool {
    required.elements().iter().all(|element| {
        !matches!(element, Value::Null)
            && candidate
                .elements()
                .iter()
                .any(|candidate| !matches!(candidate, Value::Null) && candidate == element)
    })
}

fn overlaps(left: &SqlArray, right: &SqlArray) -> bool {
    left.elements().iter().any(|element| {
        !matches!(element, Value::Null)
            && right
                .elements()
                .iter()
                .any(|other| !matches!(other, Value::Null) && other == element)
    })
}

pub(super) fn concatenate(left: &SqlArray, right: &SqlArray) -> Result<SqlArray, QueryError> {
    if left.is_empty() {
        return Ok(right.clone());
    }
    if right.is_empty() {
        return Ok(left.clone());
    }
    if left.dimensions().is_empty() || right.dimensions().is_empty() {
        return Err(unsupported(&BinaryOp::Concat));
    }
    if left.dimensions().len() + 1 == right.dimensions().len() {
        return concatenate_slice(left, right, true);
    }
    if right.dimensions().len() + 1 == left.dimensions().len() {
        return concatenate_slice(right, left, false);
    }
    if left.dimensions().len() != right.dimensions().len() {
        return Err(unsupported(&BinaryOp::Concat));
    }
    if left.dimensions()[1..]
        .iter()
        .zip(&right.dimensions()[1..])
        .any(|(left, right)| left != right)
    {
        return Err(unsupported(&BinaryOp::Concat));
    }
    let first = left.dimensions()[0];
    let other = right.dimensions()[0];
    let length = first
        .length
        .checked_add(other.length)
        .ok_or_else(|| unsupported(&BinaryOp::Concat))?;
    let lower_bound = first.lower_bound;
    let mut dimensions = left.dimensions().to_vec();
    dimensions[0] = ArrayDimension {
        length,
        lower_bound,
    };
    let elements = left
        .elements()
        .iter()
        .chain(right.elements())
        .cloned()
        .collect::<Vec<Value>>();
    SqlArray::new(dimensions, elements)
        .map_err(|error| QueryError::Cassie(crate::app::CassieError::ArrayInput(error)))
}

/// Concatenate a lower-rank array as one slice of the higher-rank value.
fn concatenate_slice(
    slice: &SqlArray,
    array: &SqlArray,
    prepend: bool,
) -> Result<SqlArray, QueryError> {
    if array.dimensions().len() < 2
        || slice.dimensions().len() + 1 != array.dimensions().len()
        || slice
            .dimensions()
            .iter()
            .zip(&array.dimensions()[1..])
            .any(|(left, right)| left != right)
    {
        return Err(unsupported(&BinaryOp::Concat));
    }
    let mut dimensions = array.dimensions().to_vec();
    dimensions[0].length = dimensions[0]
        .length
        .checked_add(1)
        .ok_or_else(|| unsupported(&BinaryOp::Concat))?;
    let elements = if prepend {
        slice
            .elements()
            .iter()
            .chain(array.elements())
            .cloned()
            .collect()
    } else {
        array
            .elements()
            .iter()
            .chain(slice.elements())
            .cloned()
            .collect()
    };
    SqlArray::new(dimensions, elements)
        .map_err(|error| QueryError::Cassie(crate::app::CassieError::ArrayInput(error)))
}

fn extend_scalar(array: &SqlArray, value: Value, prepend: bool) -> Result<SqlArray, QueryError> {
    if array.is_empty() {
        return SqlArray::from_elements(vec![value])
            .map_err(|error| QueryError::Cassie(crate::app::CassieError::ArrayInput(error)));
    }
    if array.dimensions().len() != 1 {
        return Err(unsupported(&BinaryOp::Concat));
    }
    let dimension = array.dimensions()[0];
    let length = dimension
        .length
        .checked_add(1)
        .ok_or_else(|| unsupported(&BinaryOp::Concat))?;
    let lower_bound = dimension.lower_bound;
    let mut elements = array.elements().to_vec();
    if prepend {
        elements.insert(0, value);
    } else {
        elements.push(value);
    }
    SqlArray::new(
        vec![ArrayDimension {
            length,
            lower_bound,
        }],
        elements,
    )
    .map_err(|error| QueryError::Cassie(crate::app::CassieError::ArrayInput(error)))
}

fn unsupported(op: &BinaryOp) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::Execution(format!(
        "array operator {} requires compatible SQL arrays",
        match op {
            BinaryOp::Concat => "||",
            _ => "operator",
        }
    )))
}

#[cfg(test)]
mod tests {
    use super::{concatenate_operand, ScalarValue, SqlArray, Value};

    #[test]
    fn should_preserve_typed_null_positions_during_array_concatenation() {
        // Arrange
        let array =
            ScalarValue::Array(SqlArray::from_elements(vec![Value::Int64(7)]).expect("array"));
        let null = ScalarValue::Null;

        // Act
        let append = concatenate_operand(&array, &null, (false, false), (false, true))
            .expect("array path")
            .expect("append");
        let prepend = concatenate_operand(&null, &array, (false, true), (false, false))
            .expect("array path")
            .expect("prepend");
        let identity = concatenate_operand(&array, &null, (false, false), (true, false))
            .expect("array path")
            .expect("identity");

        // Assert
        assert_eq!(
            append.to_value(),
            Value::Array(
                SqlArray::from_elements(vec![Value::Int64(7), Value::Null]).expect("expected")
            )
        );
        let Value::Array(prepend) = prepend.to_value() else {
            panic!("expected array");
        };
        assert_eq!(prepend.elements(), &[Value::Null, Value::Int64(7)]);
        assert_eq!(prepend.dimensions()[0].lower_bound, 1);
        assert_eq!(identity.to_value(), array.to_value());
    }
}
