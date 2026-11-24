//! Admitted and interruptible metadata accounting for scalar windows.

use super::BatchRow;
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::DataType;
use std::mem::size_of;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TypeCountProgress {
    Collect,
    Sift,
    Array,
}

struct Directory<'a> {
    values: Vec<&'a Vec<DataType>>,
    _memory: QueryMemoryReservation,
}

pub(crate) fn row_type_bytes_controlled(
    rows: &[BatchRow],
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    row_type_bytes_controlled_observed(rows, controls, |_| {})
}

pub(super) fn row_type_bytes_controlled_observed(
    rows: &[BatchRow],
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(TypeCountProgress),
) -> Result<usize, CassieError> {
    let mut count = 0;
    for row in rows {
        controls.check_interruption()?;
        if row.data_types.is_some() {
            count = add(count, 1)?;
        }
    }
    let admission = controls.reserve_query_memory(0)?;
    let (values, memory) = crate::runtime::owned_memory::try_reserve_vec(count, &admission)?;
    let mut directory = Directory {
        values,
        _memory: memory,
    };
    for row in rows {
        controls.check_interruption()?;
        if let Some(types) = &row.data_types {
            directory.values.push(types.as_ref());
            observe(TypeCountProgress::Collect);
            controls.check_interruption()?;
        }
    }
    sort(&mut directory.values, controls, &mut observe)?;

    let mut bytes = 0;
    let mut previous = None;
    for types in &directory.values {
        controls.check_interruption()?;
        let pointer = types.as_ptr();
        if previous == Some(pointer) {
            continue;
        }
        previous = Some(pointer);
        let mut retained = vector_bytes(types.capacity())?;
        for data_type in *types {
            controls.check_interruption()?;
            let mut current = data_type;
            while let DataType::Array(element) = current {
                retained = add(retained, size_of::<DataType>())?;
                current = element;
                observe(TypeCountProgress::Array);
                controls.check_interruption()?;
            }
        }
        bytes = add(bytes, retained)?;
    }
    controls.check_interruption()?;
    Ok(bytes)
}

pub(super) fn vector_bytes(capacity: usize) -> Result<usize, CassieError> {
    add(
        add(
            mul(capacity, size_of::<DataType>())?,
            size_of::<Vec<DataType>>(),
        )?,
        2 * size_of::<usize>(),
    )
}

pub(super) fn sort(
    values: &mut [&Vec<DataType>],
    controls: &QueryExecutionControls,
    observe: &mut impl FnMut(TypeCountProgress),
) -> Result<(), CassieError> {
    let len = values.len();
    for root in (0..len / 2).rev() {
        sift(values, root, len, controls, observe)?;
    }
    for end in (1..len).rev() {
        controls.check_interruption()?;
        values.swap(0, end);
        sift(values, 0, end, controls, observe)?;
    }
    Ok(())
}

fn sift(
    values: &mut [&Vec<DataType>],
    mut root: usize,
    end: usize,
    controls: &QueryExecutionControls,
    observe: &mut impl FnMut(TypeCountProgress),
) -> Result<(), CassieError> {
    while root < end / 2 {
        controls.check_interruption()?;
        let mut child = 2 * root + 1;
        if child + 1 < end
            && (values[child].as_ptr() as usize) < (values[child + 1].as_ptr() as usize)
        {
            child += 1;
        }
        observe(TypeCountProgress::Sift);
        controls.check_interruption()?;
        if (values[root].as_ptr() as usize) >= (values[child].as_ptr() as usize) {
            break;
        }
        values.swap(root, child);
        root = child;
    }
    Ok(())
}
