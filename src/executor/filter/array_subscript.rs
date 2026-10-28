//! Runtime SQL array subscripting and slicing.
use super::{eval_scalar_with_context, EvalContext, Expr, QueryError, RowAccess, ScalarValue};
use crate::sql::ast::ArraySubscript;
use crate::types::{ArrayDimension, SqlArray, Value};

pub(super) fn evaluate<R: RowAccess + ?Sized>(
    row: &R,
    array_expr: &Expr,
    subscripts: &[ArraySubscript],
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let ScalarValue::Array(array) = eval_scalar_with_context(row, array_expr, context)? else {
        return Ok(ScalarValue::Null);
    };
    if array.is_empty() {
        return Ok(ScalarValue::Null);
    }
    if subscripts.len() > array.dimensions().len() {
        return Err(crate::app::CassieError::ArrayInput(
            "number of array dimensions exceeds the number of subscripts".to_string(),
        )
        .into());
    }

    let mut fixed = Vec::with_capacity(subscripts.len());
    let mut ranges = Vec::with_capacity(array.dimensions().len());
    let mut has_slice = false;
    for (dimension, subscript) in array.dimensions().iter().zip(subscripts) {
        match subscript {
            ArraySubscript::Index(expr) => {
                let value = eval_scalar_with_context(row, expr, context)?;
                let ScalarValue::Int(index) = value else {
                    return Ok(ScalarValue::Null);
                };
                let index = i32::try_from(index).ok();
                if index.is_none_or(|index| {
                    index < dimension.lower_bound
                        || index > dimension.lower_bound.saturating_add(dimension.length - 1)
                }) {
                    return Ok(ScalarValue::Null);
                }
                fixed.push(index);
                ranges.push(None);
            }
            ArraySubscript::Slice { lower, upper } => {
                has_slice = true;
                fixed.push(None);
                let lower = optional_bound(row, lower.as_ref(), dimension.lower_bound, context)?;
                let upper = optional_bound(
                    row,
                    upper.as_ref(),
                    dimension.lower_bound.saturating_add(dimension.length - 1),
                    context,
                )?;
                let min = dimension.lower_bound;
                let max = min.saturating_add(dimension.length - 1);
                let start = lower.max(min);
                let end = upper.min(max);
                ranges.push((start <= end).then_some((start, end)));
            }
        }
    }

    let fully_indexed = !has_slice && subscripts.len() == array.dimensions().len();
    if fully_indexed {
        let indices = fixed.into_iter().flatten().collect::<Vec<_>>();
        let Some(offset) = array.offset(&indices) else {
            return Ok(ScalarValue::Null);
        };
        return Ok(value_to_scalar(array.elements()[offset].clone()));
    }

    slice_result(&array, &fixed, &ranges)
}

fn slice_result(
    array: &SqlArray,
    fixed: &[Option<i32>],
    ranges: &[Option<(i32, i32)>],
) -> Result<ScalarValue, QueryError> {
    let mut output_dimensions = Vec::new();
    for (index, dimension) in array.dimensions().iter().enumerate() {
        if fixed.get(index).copied().flatten().is_some() {
        } else {
            let range = ranges.get(index).copied().flatten().unwrap_or((
                dimension.lower_bound,
                dimension.lower_bound.saturating_add(dimension.length - 1),
            ));
            output_dimensions.push(ArrayDimension {
                length: range.1 - range.0 + 1,
                lower_bound: range.0,
            });
        }
    }
    if output_dimensions
        .iter()
        .any(|dimension| dimension.length <= 0)
    {
        return Ok(ScalarValue::Array(SqlArray::empty()));
    }
    let mut elements = Vec::new();
    for (offset, element) in array.elements().iter().enumerate() {
        let coordinates = coordinates(offset, array.dimensions());
        let selected = coordinates
            .iter()
            .enumerate()
            .all(|(dimension_index, coordinate)| {
                if let Some(Some(index)) = fixed.get(dimension_index) {
                    coordinate == index
                } else {
                    let (start, end) = ranges
                        .get(dimension_index)
                        .copied()
                        .flatten()
                        .unwrap_or_else(|| {
                            let dimension = array.dimensions()[dimension_index];
                            (
                                dimension.lower_bound,
                                dimension.lower_bound + dimension.length - 1,
                            )
                        });
                    (start..=end).contains(coordinate)
                }
            });
        if selected {
            elements.push(element.clone());
        }
    }
    SqlArray::new(output_dimensions, elements)
        .map(ScalarValue::Array)
        .map_err(|message| crate::app::CassieError::ArrayInput(message).into())
}

fn optional_bound<R: RowAccess + ?Sized>(
    row: &R,
    expression: Option<&Expr>,
    default: i32,
    context: EvalContext<'_>,
) -> Result<i32, QueryError> {
    let Some(expression) = expression else {
        return Ok(default);
    };
    match eval_scalar_with_context(row, expression, context)? {
        ScalarValue::Int(value) => i32::try_from(value).map_err(|_| {
            crate::app::CassieError::ArrayInput("array subscript is out of range".to_string())
                .into()
        }),
        ScalarValue::Null => Ok(default),
        _ => Err(crate::app::CassieError::ArrayInput(
            "array subscript must be an integer".to_string(),
        )
        .into()),
    }
}

fn coordinates(mut offset: usize, dimensions: &[ArrayDimension]) -> Vec<i32> {
    let mut coordinates = vec![0; dimensions.len()];
    for index in (0..dimensions.len()).rev() {
        let dimension = dimensions[index];
        let length = usize::try_from(dimension.length).unwrap_or(1);
        coordinates[index] = dimension.lower_bound + i32::try_from(offset % length).unwrap_or(0);
        offset /= length;
    }
    coordinates
}

fn value_to_scalar(value: Value) -> ScalarValue {
    match value {
        Value::Null => ScalarValue::Null,
        Value::Bool(value) => ScalarValue::Bool(value),
        Value::Int64(value) => ScalarValue::Int(value),
        Value::Float64(value) => ScalarValue::Float(value),
        Value::String(value) => ScalarValue::Str(value),
        Value::Json(value) => ScalarValue::Json(value.to_string()),
        Value::Typed(value) => ScalarValue::Typed(value),
        Value::Vector(value) => ScalarValue::Json(serde_json::json!(value.values).to_string()),
        Value::Array(value) => ScalarValue::Array(value),
    }
}
