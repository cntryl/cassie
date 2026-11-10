//! Proven output-directory bound for controlled native ARRAY(Vector) keys.
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::executor::semantic::SemanticValue;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};

#[cfg(test)]
#[path = "sort_vector_tests.rs"]
mod tests;

pub(in crate::executor) fn bound(
    value: &Value,
    data_type: Option<&DataType>,
    controls: &QueryExecutionControls,
) -> Result<Option<usize>, CassieError> {
    let (Value::Array(array), Some(DataType::Array(element_type))) = (value, data_type) else {
        return Ok(None);
    };
    if !matches!(element_type.as_ref(), DataType::Vector(_)) {
        return Ok(None);
    }
    let mut bytes = add(
        mul(array.dimensions().len(), std::mem::size_of::<(i32, i32)>())?,
        mul(array.elements().len(), std::mem::size_of::<SemanticValue>())?,
    )?;
    for element in array.elements() {
        controls.check_interruption()?;
        match element {
            Value::Null => {}
            Value::Vector(vector) => {
                bytes = add(
                    bytes,
                    mul(vector.values.len(), std::mem::size_of::<SemanticValue>())?,
                )?;
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(bytes))
}

pub(super) fn build(
    value: &Value,
    controls: &QueryExecutionControls,
) -> Result<(SemanticValue, usize), CassieError> {
    build_observing(value, controls, |_| {})
}

fn build_observing(
    value: &Value,
    controls: &QueryExecutionControls,
    mut observe_component: impl FnMut(usize),
) -> Result<(SemanticValue, usize), CassieError> {
    let Value::Array(array) = value else {
        return Err(invalid_shape());
    };
    let mut dimensions = Vec::new();
    reserve_exact(&mut dimensions, array.dimensions().len())?;
    for dimension in array.dimensions() {
        controls.check_interruption()?;
        dimensions.push((dimension.length, dimension.lower_bound));
    }
    let mut elements = Vec::new();
    reserve_exact(&mut elements, array.elements().len())?;
    let mut retained = add(
        mul(dimensions.capacity(), std::mem::size_of::<(i32, i32)>())?,
        mul(elements.capacity(), std::mem::size_of::<SemanticValue>())?,
    )?;
    for element in array.elements() {
        controls.check_interruption()?;
        match element {
            Value::Null => elements.push(SemanticValue::Null),
            Value::Vector(vector) => {
                let mut components = Vec::new();
                reserve_exact(&mut components, vector.values.len())?;
                retained = add(
                    retained,
                    mul(components.capacity(), std::mem::size_of::<SemanticValue>())?,
                )?;
                for (index, component) in vector.values.iter().enumerate() {
                    controls.check_interruption()?;
                    components.push(SemanticValue::from_value(&Value::Float64(f64::from(
                        *component,
                    ))));
                    observe_component(index);
                    controls.check_interruption()?;
                }
                elements.push(SemanticValue::Array(components));
            }
            _ => return Err(invalid_shape()),
        }
    }
    Ok((
        SemanticValue::SqlArray {
            dimensions,
            elements,
        },
        retained,
    ))
}

fn reserve_exact<T>(values: &mut Vec<T>, count: usize) -> Result<(), CassieError> {
    values
        .try_reserve_exact(count)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if values.capacity() > count {
        return Err(CassieError::ResourceLimit(
            "sort vector key exceeded its admitted directory".into(),
        ));
    }
    Ok(())
}

fn invalid_shape() -> CassieError {
    CassieError::Execution("invalid controlled vector-array key shape".into())
}
