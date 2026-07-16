use super::{
    AggregateAccumulator, BTreeMap, BatchRow, PartialAggregateGroup, QueryError,
    QueryExecutionControls, Value,
};

type Reservation = crate::runtime::QueryMemoryReservation;
type SerialGroups = BTreeMap<String, (Vec<(String, Value)>, Vec<BatchRow>)>;

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
                .len()
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
    groups: &BTreeMap<String, PartialAggregateGroup>,
) -> Result<Reservation, QueryError> {
    drop(previous);
    let bytes = groups
        .iter()
        .map(|(signature, group)| {
            signature
                .len()
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
