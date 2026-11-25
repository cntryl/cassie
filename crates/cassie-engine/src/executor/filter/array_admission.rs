//! Source-expression scratch bounds over already evaluated native array inputs.
use super::super::EvalContext;
use super::{QueryError, Value};
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use std::mem::size_of;

pub(super) fn admit(
    name: &str,
    args: &[Value],
    context: EvalContext<'_>,
) -> Result<(), QueryError> {
    let Some(memory) = context.memory else {
        return Ok(());
    };
    let known = matches!(
        name,
        "_cassie_array_value"
            | "cardinality"
            | "array_length"
            | "array_cat"
            | "array_append"
            | "array_prepend"
            | "array_fill"
            | "array_to_string"
            | "string_to_array"
            | "array_dims"
            | "array_lower"
            | "array_upper"
            | "array_position"
            | "array_positions"
            | "array_ndims"
            | "array_reverse"
            | "array_sort"
            | "array_shuffle"
            | "array_sample"
            | "trim_array"
    );
    if !known {
        return Ok(());
    }
    if let Some(controls) = context.controls {
        controls.check_interruption()?;
    }
    let mut bytes = args.iter().try_fold(1024, |bytes, value| {
        add(bytes, mul(value_clone_bytes(value)?, 4)?)
    })?;
    if matches!(name, "_cassie_array_value" | "string_to_array") {
        if let Some(Value::String(text)) = args.first() {
            bytes = add(bytes, mul(text.len(), 128)?)?;
        }
    }
    if name == "array_fill" && !args.iter().skip(1).any(Value::is_null) {
        if let [fill, Value::Array(shape), ..] = args {
            let count = if shape.elements().is_empty() {
                Some(0)
            } else {
                shape.elements().iter().try_fold(1_usize, |count, length| {
                    let Value::Int64(length) = length else {
                        return None;
                    };
                    count.checked_mul(usize::try_from(*length).ok()?)
                })
            };
            if let Some(count) = count {
                bytes = add(
                    bytes,
                    mul(
                        mul(count, add(size_of::<Value>(), value_clone_bytes(fill)?)?)?,
                        4,
                    )?,
                )?;
            }
        }
    }
    memory.grow(bytes)
}
