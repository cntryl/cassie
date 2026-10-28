//! Scalar functions over the native SQL array value.
use super::{QueryError, Value};
use std::fmt::Write as _;

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    match name {
        "_cassie_array_value" => Some(array_value(args)),
        "cardinality" => Some(cardinality(args)),
        "array_length" => Some(array_length(args)),
        "array_cat" => Some(array_cat(args)),
        "array_append" => Some(array_append(args)),
        "array_prepend" => Some(array_prepend(args)),
        "array_fill" => Some(array_fill(args)),
        "array_to_string" => Some(array_to_string(args)),
        "string_to_array" => Some(string_to_array(args)),
        "array_dims" => Some(array_dims(args)),
        "array_lower" => Some(array_bound(args, false)),
        "array_upper" => Some(array_bound(args, true)),
        "array_position" => Some(array_position(args)),
        "array_positions" => Some(array_positions(args)),
        "array_ndims" => Some(array_ndims(args)),
        "array_reverse" => Some(array_reverse(args)),
        "array_sort" => Some(array_sort(args)),
        "array_shuffle" => Some(array_shuffle(args)),
        "array_sample" => Some(array_sample(args)),
        "trim_array" => Some(trim_array(args)),
        _ => None,
    }
}

fn array_value(args: &[Value]) -> Result<Value, QueryError> {
    let [Value::String(serialized)] = args else {
        return Err(invalid_call("array value"));
    };
    let storage: serde_json::Value = serde_json::from_str(serialized).map_err(|error| {
        QueryError::Cassie(crate::app::CassieError::ArrayInput(error.to_string()))
    })?;
    crate::types::SqlArray::from_storage_json(&storage)
        .map(Value::Array)
        .map_err(|error| QueryError::Cassie(crate::app::CassieError::ArrayInput(error)))
}

fn array_cat(args: &[Value]) -> Result<Value, QueryError> {
    let [left, right] = args else {
        return Err(invalid_call("array_cat"));
    };
    match (left, right) {
        (Value::Null, Value::Null) => Ok(Value::Null),
        (Value::Null, Value::Array(array)) | (Value::Array(array), Value::Null) => {
            Ok(Value::Array(array.clone()))
        }
        (Value::Array(left), Value::Array(right)) => {
            super::super::array_operators::concatenate(left, right).map(Value::Array)
        }
        _ => Err(invalid_call("array_cat")),
    }
}

fn array_append(args: &[Value]) -> Result<Value, QueryError> {
    let [array, element] = args else {
        return Err(invalid_call("array_append"));
    };
    let source = match array {
        Value::Array(array) => array.clone(),
        Value::Null => crate::types::SqlArray::empty(),
        _ => return Err(invalid_call("array_append")),
    };
    append_or_prepend(&source, element.clone(), false, "array_append")
}

fn array_prepend(args: &[Value]) -> Result<Value, QueryError> {
    let [element, array] = args else {
        return Err(invalid_call("array_prepend"));
    };
    let source = match array {
        Value::Array(array) => array.clone(),
        Value::Null => crate::types::SqlArray::empty(),
        _ => return Err(invalid_call("array_prepend")),
    };
    append_or_prepend(&source, element.clone(), true, "array_prepend")
}

fn append_or_prepend(
    source: &crate::types::SqlArray,
    element: Value,
    prepend: bool,
    function: &str,
) -> Result<Value, QueryError> {
    if source.dimensions().len() > 1 {
        return Err(crate::app::CassieError::Unsupported(format!(
            "{function} only supports one-dimensional arrays"
        ))
        .into());
    }
    let mut values = source.elements().to_vec();
    let mut dimensions = source.dimensions().to_vec();
    if prepend {
        values.insert(0, element);
        if let Some(first) = dimensions.first_mut() {
            first.length = first
                .length
                .checked_add(1)
                .ok_or_else(|| invalid_call(function))?;
            first.lower_bound = first
                .lower_bound
                .checked_sub(1)
                .ok_or_else(|| invalid_call(function))?;
        } else {
            dimensions.push(crate::types::ArrayDimension {
                length: 1,
                lower_bound: 1,
            });
        }
    } else {
        values.push(element);
        if let Some(first) = dimensions.first_mut() {
            first.length = first
                .length
                .checked_add(1)
                .ok_or_else(|| invalid_call(function))?;
        } else {
            dimensions.push(crate::types::ArrayDimension {
                length: 1,
                lower_bound: 1,
            });
        }
    }
    crate::types::SqlArray::new(dimensions, values)
        .map(Value::Array)
        .map_err(|_| invalid_call(function))
}

fn array_fill(args: &[Value]) -> Result<Value, QueryError> {
    let [fill, dimensions, lower_bounds @ ..] = args else {
        return Err(invalid_call("array_fill"));
    };
    if fill.is_null() || dimensions.is_null() || lower_bounds.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    let Value::Array(shape) = dimensions else {
        return Err(invalid_call("array_fill"));
    };
    if shape.dimensions().len() > 1 || lower_bounds.len() > 1 {
        return Err(invalid_call("array_fill"));
    }
    let lengths = shape
        .elements()
        .iter()
        .map(|value| match value {
            Value::Int64(length) if *length >= 0 => i32::try_from(*length).map_err(|_| {
                crate::app::CassieError::ArrayInput(
                    "array_fill dimensions must be nonnegative integers".into(),
                )
            }),
            _ => Err(crate::app::CassieError::ArrayInput(
                "array_fill dimensions must be nonnegative integers".into(),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bounds = if let Some(Value::Array(bounds)) = lower_bounds.first() {
        if bounds.dimensions().len() > 1 || bounds.elements().len() != lengths.len() {
            return Err(crate::app::CassieError::ArrayInput(
                "array_fill lower bounds must match dimensions".into(),
            )
            .into());
        }
        bounds
            .elements()
            .iter()
            .map(|value| match value {
                Value::Int64(bound) => i32::try_from(*bound).map_err(|_| {
                    crate::app::CassieError::ArrayInput(
                        "array_fill lower bounds must be integers".into(),
                    )
                }),
                _ => Err(crate::app::CassieError::ArrayInput(
                    "array_fill lower bounds must be integers".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?
    } else if lower_bounds.is_empty() {
        vec![1; lengths.len()]
    } else {
        return Err(invalid_call("array_fill"));
    };
    let dimensions = lengths
        .into_iter()
        .zip(bounds)
        .map(|(length, lower_bound)| crate::types::ArrayDimension {
            length,
            lower_bound,
        })
        .collect::<Vec<_>>();
    let count = dimensions
        .iter()
        .try_fold(1_usize, |count, dimension| {
            usize::try_from(dimension.length)
                .ok()
                .and_then(|length| count.checked_mul(length))
        })
        .ok_or_else(|| {
            crate::app::CassieError::ArrayInput(
                "array_fill dimensions exceed supported size".into(),
            )
        })?;
    let mut elements = Vec::new();
    elements.try_reserve_exact(count).map_err(|_| {
        crate::app::CassieError::ArrayInput("array_fill dimensions exceed available memory".into())
    })?;
    elements.resize(count, fill.clone());
    crate::types::SqlArray::new(dimensions, elements)
        .map(Value::Array)
        .map_err(|message| crate::app::CassieError::ArrayInput(message).into())
}

fn array_to_string(args: &[Value]) -> Result<Value, QueryError> {
    let [array, delimiter, null_string @ ..] = args else {
        return Err(invalid_call("array_to_string"));
    };
    let (Value::Array(array), Value::String(delimiter)) = (array, delimiter) else {
        return if array.is_null() || delimiter.is_null() {
            Ok(Value::Null)
        } else {
            Err(invalid_call("array_to_string"))
        };
    };
    if null_string.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    let null_string = match null_string {
        [] => None,
        [Value::String(value)] => Some(value.as_str()),
        _ => return Err(invalid_call("array_to_string")),
    };
    let mut output = String::new();
    let mut emitted = false;
    for value in array.elements() {
        let text = if value.is_null() {
            let Some(null_string) = null_string else {
                continue;
            };
            null_string.to_owned()
        } else {
            value_text(value).ok_or_else(|| invalid_call("array_to_string"))?
        };
        if emitted {
            output.push_str(delimiter);
        }
        emitted = true;
        output.push_str(&text);
    }
    Ok(Value::String(output))
}

fn string_to_array(args: &[Value]) -> Result<Value, QueryError> {
    let [input, delimiter, null_string @ ..] = args else {
        return Err(invalid_call("string_to_array"));
    };
    if input.is_null() {
        return Ok(Value::Null);
    }
    let Value::String(input) = input else {
        return Err(invalid_call("string_to_array"));
    };
    if null_string.len() > 1 {
        return Err(invalid_call("string_to_array"));
    }
    let null_string = match null_string {
        [] | [Value::Null] => None,
        [Value::String(value)] => Some(value.as_str()),
        _ => return Err(invalid_call("string_to_array")),
    };
    let parts = match delimiter {
        Value::Null => input
            .chars()
            .map(|character| character.to_string())
            .collect::<Vec<_>>(),
        Value::String(delimiter) if delimiter.is_empty() => {
            if input.is_empty() {
                Vec::new()
            } else {
                vec![input.clone()]
            }
        }
        Value::String(delimiter) => input.split(delimiter).map(str::to_owned).collect(),
        _ => return Err(invalid_call("string_to_array")),
    };
    let values = parts
        .into_iter()
        .map(|part| {
            if null_string.is_some_and(|null_string| null_string == part) {
                Value::Null
            } else {
                Value::String(part)
            }
        })
        .collect();
    crate::types::SqlArray::from_elements(values)
        .map(Value::Array)
        .map_err(|_| invalid_call("string_to_array"))
}

fn value_text(value: &Value) -> Option<String> {
    Some(match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => if *value { "t" } else { "f" }.to_owned(),
        Value::Int64(value) => value.to_string(),
        Value::Float64(value) => crate::types::float_text::format(*value),
        Value::Json(value) => value.to_string(),
        Value::Typed(value) => value.text(),
        Value::Vector(value) => format!(
            "[{}]",
            value
                .values
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Array(_) | Value::Null => return None,
    })
}

fn cardinality(args: &[Value]) -> Result<Value, QueryError> {
    let [value] = args else {
        return Err(invalid_call("cardinality"));
    };
    match value {
        Value::Null => Ok(Value::Null),
        Value::Array(array) => i64::try_from(array.len())
            .map(Value::Int64)
            .map_err(|_| invalid_call("cardinality")),
        _ => Err(invalid_call("cardinality")),
    }
}

fn array_length(args: &[Value]) -> Result<Value, QueryError> {
    let [value, dimension] = args else {
        return Err(invalid_call("array_length"));
    };
    if matches!(value, Value::Null) || matches!(dimension, Value::Null) {
        return Ok(Value::Null);
    }
    let (Value::Array(array), Value::Int64(dimension)) = (value, dimension) else {
        return Err(invalid_call("array_length"));
    };
    if *dimension <= 0 {
        return Err(crate::app::CassieError::ArrayInput(
            "array_subscript_error: dimension must be greater than zero".to_string(),
        )
        .into());
    }
    Ok(array
        .dimensions()
        .get(usize::try_from(*dimension - 1).unwrap_or(usize::MAX))
        .map_or(Value::Null, |dimension| {
            Value::Int64(i64::from(dimension.length))
        }))
}

fn array_dims(args: &[Value]) -> Result<Value, QueryError> {
    let [value] = args else {
        return Err(invalid_call("array_dims"));
    };
    match value {
        Value::Null => Ok(Value::Null),
        Value::Array(array) if array.is_empty() => Ok(Value::Null),
        Value::Array(array) => {
            let mut dimensions = String::new();
            for dimension in array.dimensions() {
                let upper = dimension
                    .lower_bound
                    .checked_add(dimension.length - 1)
                    .ok_or_else(|| invalid_call("array_dims"))?;
                let _ = write!(dimensions, "[{}:{upper}]", dimension.lower_bound);
            }
            Ok(Value::String(dimensions))
        }
        _ => Err(invalid_call("array_dims")),
    }
}

fn array_bound(args: &[Value], upper: bool) -> Result<Value, QueryError> {
    let name = if upper { "array_upper" } else { "array_lower" };
    let [value, dimension] = args else {
        return Err(invalid_call(name));
    };
    if matches!(value, Value::Null) || matches!(dimension, Value::Null) {
        return Ok(Value::Null);
    }
    let (Value::Array(array), Value::Int64(dimension)) = (value, dimension) else {
        return Err(invalid_call(name));
    };
    let index = dimension_index(*dimension)?;
    let Some(dimension) = array.dimensions().get(index) else {
        return Ok(Value::Null);
    };
    let bound = if upper {
        dimension
            .lower_bound
            .checked_add(dimension.length - 1)
            .ok_or_else(|| invalid_call(name))?
    } else {
        dimension.lower_bound
    };
    Ok(Value::Int64(i64::from(bound)))
}

fn array_position(args: &[Value]) -> Result<Value, QueryError> {
    let [array, search, rest @ ..] = args else {
        return Err(invalid_call("array_position"));
    };
    let start = match rest {
        [] => None,
        [Value::Null] => {
            return Err(crate::app::CassieError::InvalidParameterValue(
                "initial position must not be null".into(),
            )
            .into());
        }
        [Value::Int64(start)] => Some(*start),
        _ => return Err(invalid_call("array_position")),
    };
    let Value::Array(array) = array else {
        return if matches!(array, Value::Null) {
            Ok(Value::Null)
        } else {
            Err(invalid_call("array_position"))
        };
    };
    check_search_dimensions(array)?;
    let Some(dimension) = array.dimensions().first() else {
        return Ok(Value::Null);
    };
    let lower_bound = i64::from(dimension.lower_bound);
    let from = start.map_or(lower_bound, |start| start.max(lower_bound));
    if from > lower_bound + i64::from(dimension.length) - 1 {
        return Ok(Value::Null);
    }
    Ok(array
        .elements()
        .iter()
        .enumerate()
        .find(|(offset, value)| {
            i64::try_from(*offset)
                .is_ok_and(|offset| i64::from(dimension.lower_bound) + offset >= from)
                && *value == search
        })
        .map_or(Value::Null, |(offset, _)| {
            Value::Int64(
                i64::from(dimension.lower_bound)
                    + i64::try_from(offset).expect("array offset fits in i64"),
            )
        }))
}

fn array_positions(args: &[Value]) -> Result<Value, QueryError> {
    let [array, search] = args else {
        return Err(invalid_call("array_positions"));
    };
    let Value::Array(array) = array else {
        return if matches!(array, Value::Null) {
            Ok(Value::Null)
        } else {
            Err(invalid_call("array_positions"))
        };
    };
    check_search_dimensions(array)?;
    let elements = array.elements();
    let Some(dimension) = array.dimensions().first() else {
        return Ok(Value::Array(crate::types::SqlArray::empty()));
    };
    let positions = elements
        .iter()
        .enumerate()
        .filter(|(_, value)| *value == search)
        .map(|(offset, _)| {
            Value::Int64(
                i64::from(dimension.lower_bound)
                    + i64::try_from(offset).expect("array offset fits in i64"),
            )
        })
        .collect();
    crate::types::SqlArray::from_elements(positions)
        .map(Value::Array)
        .map_err(|error| crate::app::CassieError::ArrayInput(error).into())
}

fn array_ndims(args: &[Value]) -> Result<Value, QueryError> {
    let [value] = args else {
        return Err(invalid_call("array_ndims"));
    };
    match value {
        Value::Null => Ok(Value::Null),
        Value::Array(array) if array.is_empty() => Ok(Value::Null),
        Value::Array(array) => i64::try_from(array.dimensions().len())
            .map(Value::Int64)
            .map_err(|_| invalid_call("array_ndims")),
        _ => Err(invalid_call("array_ndims")),
    }
}

fn array_reverse(args: &[Value]) -> Result<Value, QueryError> {
    let [value] = args else {
        return Err(invalid_call("array_reverse"));
    };
    let Value::Array(array) = value else {
        return Ok(value.clone());
    };
    let Some(first) = array.dimensions().first() else {
        return Ok(value.clone());
    };
    let row_width = array
        .dimensions()
        .iter()
        .skip(1)
        .try_fold(1_usize, |size, dimension| {
            usize::try_from(dimension.length)
                .ok()
                .and_then(|length| size.checked_mul(length))
        })
        .ok_or_else(|| invalid_call("array_reverse"))?;
    let mut elements = array.elements().to_vec();
    let length = usize::try_from(first.length).map_err(|_| invalid_call("array_reverse"))?;
    for left in 0..length / 2 {
        let right = length - left - 1;
        for offset in 0..row_width {
            elements.swap(left * row_width + offset, right * row_width + offset);
        }
    }
    shaped_array(
        array,
        array.dimensions().to_vec(),
        elements,
        "array_reverse",
    )
}

fn array_sort(args: &[Value]) -> Result<Value, QueryError> {
    let [value, rest @ ..] = args else {
        return Err(invalid_call("array_sort"));
    };
    if args.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    let Value::Array(array) = value else {
        return Ok(value.clone());
    };
    if array.dimensions().len() > 1 {
        return Err(crate::app::CassieError::Unsupported(
            "array_sort does not support multidimensional arrays".into(),
        )
        .into());
    }
    let descending = option_bool(rest.first(), false, "array_sort")?;
    let nulls_first = option_bool(rest.get(1), false, "array_sort")?;
    let mut elements = array.elements().to_vec();
    elements.sort_by(|left, right| match (left.is_null(), right.is_null()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => {
            if nulls_first {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        }
        (false, true) => {
            if nulls_first {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        }
        (false, false) => {
            let order = crate::types::semantic::compare_values(left, right);
            if descending {
                order.reverse()
            } else {
                order
            }
        }
    });
    shaped_array(array, array.dimensions().to_vec(), elements, "array_sort")
}

fn array_shuffle(args: &[Value]) -> Result<Value, QueryError> {
    let [value] = args else {
        return Err(invalid_call("array_shuffle"));
    };
    let Value::Array(array) = value else {
        return Ok(value.clone());
    };
    randomize_outer_dimension(array)
}

fn array_sample(args: &[Value]) -> Result<Value, QueryError> {
    let [value, count] = args else {
        return Err(invalid_call("array_sample"));
    };
    let Value::Array(array) = value else {
        return Ok(value.clone());
    };
    let Value::Int64(count) = count else {
        return if count.is_null() {
            Ok(Value::Null)
        } else {
            Err(invalid_call("array_sample"))
        };
    };
    if *count < 0 {
        return Err(crate::app::CassieError::InvalidParameterValue(
            "sample size must not be negative".into(),
        )
        .into());
    }
    if array.dimensions().len() > 1 {
        return Err(crate::app::CassieError::Unsupported(
            "array_sample does not support multidimensional arrays".into(),
        )
        .into());
    }
    let mut shuffled = array.elements().to_vec();
    shuffle(&mut shuffled);
    let keep = usize::try_from(*count)
        .unwrap_or(usize::MAX)
        .min(shuffled.len());
    shuffled.truncate(keep);
    let mut dimensions = array.dimensions().to_vec();
    if let Some(first) = dimensions.first_mut() {
        first.length = i32::try_from(keep).map_err(|_| invalid_call("array_sample"))?;
        // Sampling constructs a new one-dimensional array with PostgreSQL's
        // canonical lower bound, even when the input array used another one.
        first.lower_bound = 1;
    }
    shaped_array(array, dimensions, shuffled, "array_sample")
}

fn trim_array(args: &[Value]) -> Result<Value, QueryError> {
    let [value, count] = args else {
        return Err(invalid_call("trim_array"));
    };
    let Value::Array(array) = value else {
        return Ok(value.clone());
    };
    let Value::Int64(count) = count else {
        return if count.is_null() {
            Ok(Value::Null)
        } else {
            Err(invalid_call("trim_array"))
        };
    };
    if *count < 0 {
        return Err(crate::app::CassieError::InvalidParameterValue(
            "trim length must not be negative".into(),
        )
        .into());
    }
    let Some(first) = array.dimensions().first() else {
        return Ok(value.clone());
    };
    let length = usize::try_from(first.length).map_err(|_| invalid_call("trim_array"))?;
    let trim = usize::try_from(*count).unwrap_or(usize::MAX).min(length);
    let keep = length - trim;
    let row_width = array
        .dimensions()
        .iter()
        .skip(1)
        .try_fold(1_usize, |size, dimension| {
            usize::try_from(dimension.length)
                .ok()
                .and_then(|length| size.checked_mul(length))
        })
        .ok_or_else(|| invalid_call("trim_array"))?;
    let keep_elements = keep
        .checked_mul(row_width)
        .ok_or_else(|| invalid_call("trim_array"))?;
    let mut dimensions = array.dimensions().to_vec();
    dimensions[0].length = i32::try_from(keep).map_err(|_| invalid_call("trim_array"))?;
    shaped_array(
        array,
        dimensions,
        array.elements()[..keep_elements].to_vec(),
        "trim_array",
    )
}

fn randomize_outer_dimension(array: &crate::types::SqlArray) -> Result<Value, QueryError> {
    let Some(first) = array.dimensions().first() else {
        return Ok(Value::Array(array.clone()));
    };
    let row_width = array
        .dimensions()
        .iter()
        .skip(1)
        .try_fold(1_usize, |size, dimension| {
            usize::try_from(dimension.length)
                .ok()
                .and_then(|length| size.checked_mul(length))
        })
        .ok_or_else(|| invalid_call("array_shuffle"))?;
    let mut indices = (0..usize::try_from(first.length)
        .map_err(|_| invalid_call("array_shuffle"))?)
        .collect::<Vec<_>>();
    shuffle(&mut indices);
    let elements = indices
        .into_iter()
        .flat_map(|index| {
            let start = index * row_width;
            array.elements()[start..start + row_width].iter().cloned()
        })
        .collect();
    shaped_array(
        array,
        array.dimensions().to_vec(),
        elements,
        "array_shuffle",
    )
}

fn shuffle<T>(values: &mut [T]) {
    let mut seed = [0_u8; 8];
    if getrandom::fill(&mut seed).is_err() {
        seed.copy_from_slice(
            &u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_nanos()),
            )
            .unwrap_or(0)
            .to_ne_bytes(),
        );
    }
    let mut state = u64::from_ne_bytes(seed);
    for index in (1..values.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        values.swap(
            index,
            usize::try_from(state % u64::try_from(index + 1).unwrap_or(u64::MAX)).unwrap_or(0),
        );
    }
}

fn option_bool(value: Option<&Value>, default: bool, function: &str) -> Result<bool, QueryError> {
    match value {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::Null) => Err(crate::app::CassieError::InvalidParameterValue(format!(
            "{function} options must not be null"
        ))
        .into()),
        _ => Err(invalid_call(function)),
    }
}

fn shaped_array(
    original: &crate::types::SqlArray,
    dimensions: Vec<crate::types::ArrayDimension>,
    elements: Vec<Value>,
    function: &str,
) -> Result<Value, QueryError> {
    if original.is_empty() && dimensions.is_empty() {
        return Ok(Value::Array(crate::types::SqlArray::empty()));
    }
    crate::types::SqlArray::new(dimensions, elements)
        .map(Value::Array)
        .map_err(|_| invalid_call(function))
}

fn check_search_dimensions(array: &crate::types::SqlArray) -> Result<(), QueryError> {
    if array.dimensions().len() > 1 {
        return Err(crate::app::CassieError::Unsupported(
            "searching for elements in multidimensional arrays is not supported".into(),
        )
        .into());
    }
    Ok(())
}

fn dimension_index(dimension: i64) -> Result<usize, QueryError> {
    if dimension <= 0 {
        return Err(crate::app::CassieError::ArrayInput(
            "array_subscript_error: dimension must be greater than zero".into(),
        )
        .into());
    }
    usize::try_from(dimension - 1).map_err(|_| invalid_call("array dimension"))
}

fn invalid_call(name: &str) -> QueryError {
    crate::app::CassieError::Execution(format!("{name} requires SQL array arguments")).into()
}
