//! Restore logical CHAR-array padding under retained query admission.
//! Durable values keep their existing canonical, trailing-space-trimmed bytes.
use std::mem::size_of;
use std::sync::Arc;

use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::executor::retained_memory::add;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{DataType, Value};

fn width(kind: &DataType) -> Option<u32> {
    let DataType::Array(element) = kind else {
        return None;
    };
    let mut element = element.as_ref();
    while let DataType::Array(inner) = element {
        element = inner.as_ref();
    }
    match element {
        DataType::Char { length } => *length,
        _ => None,
    }
}

fn plan(text: &str, width: u32) -> Result<crate::types::char_text::CharacterPlan, CassieError> {
    crate::types::char_text::conversion_plan(text, Some(width), true, false)
        .map_err(CassieError::TypeInput)
}

fn padding_bytes(
    value: &Value,
    kind: &DataType,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    let (Some(width), Value::Array(array)) = (width(kind), value) else {
        return Ok(0);
    };
    array.elements().iter().try_fold(0, |bytes, value| {
        controls.check_interruption()?;
        let Value::String(text) = value else {
            return Ok(bytes);
        };
        let plan = plan(text, width)?;
        add(
            bytes,
            if plan.padding == 0 {
                0
            } else {
                plan.output_bytes
            },
        )
    })
}

/// The caller owns the original carrier and keeps this lease until the native
/// destination has admitted its own copy. Admit every replacement before growth.
pub(crate) fn hydrate_value(
    value: &mut Value,
    kind: &DataType,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    memory.try_grow(padding_bytes(value, kind, controls)?)?;
    hydrate(value, kind, controls)
}

pub(crate) fn hydrate_row(
    mut row: BatchRow,
    controls: &QueryExecutionControls,
) -> Result<BatchRow, CassieError> {
    controls.check_interruption()?;
    let Some(types) = row.shared_data_types() else {
        return Ok(row);
    };
    let bytes =
        row.entries()
            .iter()
            .zip(types.iter())
            .try_fold(0, |bytes, ((_, value), kind)| {
                controls.check_interruption()?;
                add(bytes, padding_bytes(value, kind, controls)?)
            })?;
    if bytes == 0 {
        return Ok(row);
    }
    let memory = controls.reserve_query_memory(add(
        add(
            bytes,
            add(row.unleased_body_bytes()?, row.pending_lookup_bytes()?)?,
        )?,
        size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
    )?)?;
    for ((_, value), kind) in row.entries_mut().iter_mut().zip(types.iter()) {
        hydrate(value, kind, controls)?;
    }
    controls.check_interruption()?;
    row.retain_operator_memory(controls, Arc::new(memory))
}

fn hydrate(
    value: &mut Value,
    kind: &DataType,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    let (Some(width), Value::Array(array)) = (width(kind), value) else {
        return Ok(());
    };
    for value in array.elements_mut() {
        controls.check_interruption()?;
        let Value::String(text) = value else {
            continue;
        };
        let plan = plan(text, width)?;
        if plan.padding == 0 {
            continue;
        }
        let mut output = String::new();
        output
            .try_reserve_exact(plan.output_bytes)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if output.capacity() > plan.output_bytes {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted logical character padding".into(),
            ));
        }
        output.push_str(&text[..plan.prefix_bytes]);
        let mut remaining = plan.padding;
        while remaining != 0 {
            controls.check_interruption()?;
            let count = remaining.min(4096);
            output.extend(std::iter::repeat_n(' ', count));
            remaining -= count;
            #[cfg(test)]
            AFTER_CHUNK.with(|slot| {
                if let Some(cancellation) = slot.borrow_mut().take() {
                    cancellation.cancel();
                }
            });
        }
        controls.check_interruption()?;
        *text = output;
        #[cfg(test)]
        HYDRATED.with(|count| count.set(count.get() + 1));
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    static HYDRATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static AFTER_CHUNK: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[path = "character_arrays_tests.rs"]
mod tests;
