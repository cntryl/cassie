//! Group identity and admission bytes for the typed grouped aggregate.
use super::QueryError;
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::executor::typed_batch::TypedBatch;
use crate::types::semantic::{SemanticKey, SemanticValue};
use crate::types::{DataType, Value};
use std::mem::size_of;

/// Grouping key types whose semantic key equals the row path's key.
///
/// `Char` is excluded: its key pads to the declared length.
pub(super) fn supported_key(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Boolean
            | DataType::Text
            | DataType::Varchar { .. }
    )
}

/// Types `COUNT(column)` can test for NULL without interpreting the value.
pub(super) fn supported_count_input(data_type: &DataType) -> bool {
    supported_key(data_type) || matches!(data_type, DataType::Char { .. })
}

/// The semantic key and the first-seen values of one lane's grouping columns.
///
/// NULL is its own group: it keys as `SemanticValue::Null`.
pub(super) fn lane_key(
    columns: &[usize],
    batch: &TypedBatch,
    lane: usize,
) -> Result<(SemanticKey, Vec<Value>), QueryError> {
    let mut values = Vec::with_capacity(columns.len());
    let mut key = Vec::with_capacity(columns.len());
    for column in columns {
        let value = batch.cell(*column, lane)?.to_owned();
        key.push(SemanticValue::from_value(&value));
        values.push(value);
    }
    Ok((SemanticKey::from_semantic_values(key), values))
}

/// Bytes one new group retains beyond the map's own index storage.
///
/// `fixed` is the caller's inline group size (record plus aggregate states).
pub(super) fn group_bytes(
    key: &SemanticKey,
    values: &[Value],
    fixed: usize,
) -> Result<usize, QueryError> {
    let heap = values
        .iter()
        .try_fold(mul(values.len(), size_of::<Value>())?, |bytes, value| {
            add(bytes, value_clone_bytes(value)?)
        })?;
    Ok(add(add(key.estimated_bytes(), heap)?, fixed)?)
}
