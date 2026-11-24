//! Keep existing aggregate group reservations through output row ownership.
use std::sync::Arc;

use crate::executor::batch::BatchRow;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{GroupMemory, QueryError};

#[cfg(test)]
#[path = "output_owner_tests.rs"]
mod tests;

struct PendingOutputs {
    rows: Vec<BatchRow>,
    shared: Option<Arc<QueryMemoryReservation>>,
    partitions: Vec<GroupMemory>,
    primary: Option<GroupMemory>,
}

pub(super) fn retain_outputs(
    rows: Vec<BatchRow>,
    primary: GroupMemory,
    partitions: Vec<GroupMemory>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    retain_outputs_observing(rows, primary, partitions, controls, |_| {})
}

fn retain_outputs_observing(
    rows: Vec<BatchRow>,
    primary: GroupMemory,
    partitions: Vec<GroupMemory>,
    controls: &QueryExecutionControls,
    mut observe_attached: impl FnMut(usize),
) -> Result<Vec<BatchRow>, QueryError> {
    let mut pending = PendingOutputs {
        rows,
        shared: None,
        partitions,
        primary: Some(primary),
    };
    if pending.rows.is_empty() {
        return Ok(Vec::new());
    }
    if !pending
        .primary
        .as_ref()
        .expect("primary owner")
        .belongs_to(controls)
        || pending
            .partitions
            .iter()
            .any(|memory| !memory.belongs_to(controls))
    {
        return Err(crate::app::CassieError::ResourceLimit(
            "aggregate group owners use different query trackers".into(),
        )
        .into());
    }
    pending
        .primary
        .as_mut()
        .expect("primary owner")
        .add(std::mem::size_of::<QueryMemoryReservation>() + 2 * std::mem::size_of::<usize>())?;
    while let Some(partition) = pending.partitions.pop() {
        pending
            .primary
            .as_mut()
            .expect("primary owner")
            .absorb(partition);
    }
    pending.shared = Some(Arc::new(
        pending
            .primary
            .take()
            .expect("primary owner")
            .into_reservation(),
    ));
    for index in 0..pending.rows.len() {
        let row = std::mem::replace(
            &mut pending.rows[index],
            BatchRow::from_projected_values(Vec::new()),
        );
        pending.rows[index] = row.retain_operator_memory(
            controls,
            Arc::clone(pending.shared.as_ref().expect("shared owner")),
        )?;
        observe_attached(index);
        super::check_timeout(controls)?;
    }
    Ok(std::mem::take(&mut pending.rows))
}
