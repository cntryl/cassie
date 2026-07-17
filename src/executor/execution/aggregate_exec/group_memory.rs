use std::collections::BTreeMap;

use crate::executor::batch::BatchRow;
use crate::executor::semantic::SemanticKey;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::state::{AggregateAccumulator, PartialAggregateGroup};
use super::QueryError;

type Reservation = crate::runtime::QueryMemoryReservation;
type SerialGroups = BTreeMap<SemanticKey, (Vec<(String, Value)>, Vec<BatchRow>)>;

pub(super) fn replace_serial(
    previous: Option<Reservation>,
    controls: &QueryExecutionControls,
    groups: &SerialGroups,
) -> Result<Reservation, QueryError> {
    drop(previous);
    let bytes = groups
        .iter()
        .map(|(signature, (values, rows))| {
            signature
                .estimated_bytes()
                .saturating_add(json_bytes(values))
                .saturating_add(
                    rows.iter()
                        .map(|row| json_bytes(row.entries()))
                        .sum::<usize>(),
                )
        })
        .sum();
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

pub(super) fn replace_partial(
    previous: Option<Reservation>,
    controls: &QueryExecutionControls,
    groups: &BTreeMap<SemanticKey, PartialAggregateGroup>,
) -> Result<Reservation, QueryError> {
    drop(previous);
    let bytes = groups
        .iter()
        .map(|(signature, group)| {
            signature
                .estimated_bytes()
                .saturating_add(json_bytes(&group.group_values))
                .saturating_add(
                    group
                        .accumulators
                        .len()
                        .saturating_mul(std::mem::size_of::<AggregateAccumulator>()),
                )
        })
        .sum();
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

fn json_bytes<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or_default()
}
