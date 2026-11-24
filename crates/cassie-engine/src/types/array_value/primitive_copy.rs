//! Checked, shape-preserving copies of primitive SQL Array carriers.

use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{ArrayDimension, SqlArray, Value};

#[derive(Clone, Copy)]
pub(crate) enum PrimitiveCopyProgress {
    Eligibility,
    Dimension,
    Element,
}

/// The array allocation must be destroyed before its reservation.
pub(crate) struct OwnedPrimitiveArray {
    value: SqlArray,
    memory: QueryMemoryReservation,
}

impl OwnedPrimitiveArray {
    pub(crate) fn into_parts(self) -> (SqlArray, QueryMemoryReservation) {
        (self.value, self.memory)
    }
}

struct PendingArray {
    dimensions: Vec<ArrayDimension>,
    elements: Vec<Value>,
    memory: QueryMemoryReservation,
}

/// Copy only fixed-size primitive carriers, preserving the exact source shape.
/// Values outside this family are left to the caller's existing fallback.
pub(crate) fn copy_primitive_array(
    array: &SqlArray,
    source: &QueryMemoryReservation,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(PrimitiveCopyProgress),
) -> Result<Option<OwnedPrimitiveArray>, CassieError> {
    if !source.belongs_to(controls) {
        return Err(CassieError::ResourceLimit(
            "SQL Array source uses a different query tracker".into(),
        ));
    }
    controls.check_interruption()?;
    for value in &array.elements {
        controls.check_interruption()?;
        observe(PrimitiveCopyProgress::Eligibility);
        controls.check_interruption()?;
        if !matches!(
            value,
            Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_)
        ) {
            return Ok(None);
        }
    }
    let bytes = add(
        mul(array.dimensions.len(), size_of::<ArrayDimension>())?,
        mul(array.elements.len(), size_of::<Value>())?,
    )?;
    let mut pending = PendingArray {
        dimensions: Vec::new(),
        elements: Vec::new(),
        memory: source.try_sibling(bytes)?,
    };
    pending
        .dimensions
        .try_reserve_exact(array.dimensions.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    pending
        .elements
        .try_reserve_exact(array.elements.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if pending.dimensions.capacity() > array.dimensions.len()
        || pending.elements.capacity() > array.elements.len()
    {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted SQL Array capacity".into(),
        ));
    }
    for dimension in &array.dimensions {
        controls.check_interruption()?;
        pending.dimensions.push(*dimension);
        observe(PrimitiveCopyProgress::Dimension);
        controls.check_interruption()?;
    }
    for element in &array.elements {
        controls.check_interruption()?;
        pending.elements.push(match element {
            Value::Null => Value::Null,
            Value::Bool(value) => Value::Bool(*value),
            Value::Int64(value) => Value::Int64(*value),
            Value::Float64(value) => Value::Float64(*value),
            _ => unreachable!("eligibility checked before copying"),
        });
        observe(PrimitiveCopyProgress::Element);
        controls.check_interruption()?;
    }
    controls.check_interruption()?;
    let PendingArray {
        dimensions,
        elements,
        memory,
    } = pending;
    Ok(Some(OwnedPrimitiveArray {
        value: SqlArray {
            dimensions,
            elements,
        },
        memory,
    }))
}
