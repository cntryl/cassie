//! Checked byte copying for scoped native BIT/VARBIT sort keys.
use crate::app::CassieError;
use crate::executor::semantic::SemanticValue;
use crate::runtime::QueryExecutionControls;
use crate::types::extended::ExtendedType;
use crate::types::typed::{TypedKey, TypedValue};
use crate::types::{DataType, Value};

#[cfg(test)]
#[path = "sort_bit_tests.rs"]
mod tests;

pub(in crate::executor) fn eligible(value: &Value, data_type: Option<&DataType>) -> bool {
    matches!(
        value,
        Value::Typed(TypedValue::Bit(_) | TypedValue::VarBit(_))
    ) && matches!(
        data_type,
        Some(DataType::Extended(
            ExtendedType::Bit { .. } | ExtendedType::VarBit { .. }
        ))
    )
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
    mut observe_chunk: impl FnMut(usize),
) -> Result<(SemanticValue, usize), CassieError> {
    let Value::Typed(TypedValue::Bit(bit) | TypedValue::VarBit(bit)) = value else {
        return Err(CassieError::Execution(
            "invalid controlled bit sort key shape".into(),
        ));
    };
    controls.check_interruption()?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(bit.bytes().len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if bytes.capacity() > bit.bytes().len() {
        return Err(CassieError::ResourceLimit(
            "bit sort key exceeded its admitted directory".into(),
        ));
    }
    for (index, chunk) in bit.bytes().chunks(1024).enumerate() {
        controls.check_interruption()?;
        bytes.extend_from_slice(chunk);
        observe_chunk(index);
        controls.check_interruption()?;
    }
    controls.check_interruption()?;
    let retained = bytes.capacity();
    Ok((
        SemanticValue::Typed(TypedKey::Bits(bytes, bit.len())),
        retained,
    ))
}
