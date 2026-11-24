//! Preserve source parents while attaching an expanded output reservation.
use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER_ATTACH: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(in crate::executor::projection) fn cancel_after_output_attachment_once(
    handle: crate::runtime::QueryCancellationHandle,
) {
    CANCEL_AFTER_ATTACH.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
fn attached() {
    CANCEL_AFTER_ATTACH.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn attached() {}

struct PendingOutputs {
    rows: Vec<BatchRow>,
    reservation: Option<QueryMemoryReservation>,
    shared: Option<Arc<QueryMemoryReservation>>,
}

pub(super) struct RetainedRows {
    pub(super) rows: Vec<BatchRow>,
    pub(super) source_owner: Option<Arc<QueryMemoryReservation>>,
}

pub(super) fn retain(
    rows: Vec<BatchRow>,
    reservation: QueryMemoryReservation,
    controls: &QueryExecutionControls,
) -> Result<RetainedRows, CassieError> {
    let mut pending = PendingOutputs {
        rows,
        reservation: Some(reservation),
        shared: None,
    };
    if pending.rows.is_empty() {
        return Ok(RetainedRows {
            rows: Vec::new(),
            source_owner: None,
        });
    }
    if !pending
        .reservation
        .as_ref()
        .expect("output reservation")
        .belongs_to(controls)
    {
        return Err(CassieError::ResourceLimit(
            "JSON object-key output uses a different query tracker".into(),
        ));
    }
    pending
        .reservation
        .as_mut()
        .expect("output reservation")
        .try_grow(
            std::mem::size_of::<QueryMemoryReservation>() + 2 * std::mem::size_of::<usize>(),
        )?;
    pending.shared = Some(Arc::new(
        pending.reservation.take().expect("output reservation"),
    ));
    for row in &mut pending.rows {
        let shared = Arc::clone(pending.shared.as_ref().expect("shared output"));
        if row.query_memory().is_none() && row.operator_memory().is_none() {
            let value = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
            *row = value.with_query_memory(Some(shared));
        } else {
            row.attach_operator_memory(controls, shared)?;
        }
        attached();
        controls.check_interruption()?;
    }
    Ok(RetainedRows {
        rows: pending.rows,
        source_owner: pending.shared,
    })
}
