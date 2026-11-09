//! Bounded SELECT-list expansion for the selected JSON object-key functions.
#[cfg(test)]
use super::BatchRow;
use super::{Batch, QueryError};
use crate::executor::batch::chunk_rows;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::SelectItem;
use crate::types::Value;

#[path = "json_object_keys_output.rs"]
mod output;
#[cfg(test)]
pub(super) use output::cancel_after_output_attachment_once;

#[path = "json_object_keys_row.rs"]
mod row;
#[path = "json_object_keys_rows.rs"]
mod rows;

#[derive(Clone, Copy)]
enum Progress {
    Name,
    Key,
    Entries,
    Slot,
    Move,
    Reverse,
}

#[cfg(test)]
thread_local! {
    static CANCEL_EXPAND: std::cell::RefCell<Option<(crate::runtime::QueryCancellationHandle, u8)>> = const { std::cell::RefCell::new(None) };
    static OBSERVE_SOURCE: std::cell::RefCell<Option<(std::sync::Weak<crate::runtime::QueryMemoryReservation>, std::rc::Rc<std::cell::Cell<bool>>)>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn cancel_expand_once(handle: crate::runtime::QueryCancellationHandle, stage: u8) {
    CANCEL_EXPAND.with(|slot| *slot.borrow_mut() = Some((handle, stage)));
}
#[cfg(test)]
pub(super) fn observe_source_during_copy(
    source: std::sync::Weak<crate::runtime::QueryMemoryReservation>,
    observed: std::rc::Rc<std::cell::Cell<bool>>,
) {
    OBSERVE_SOURCE.with(|slot| *slot.borrow_mut() = Some((source, observed)));
}
#[cfg(test)]
fn observe(progress: Progress) {
    if matches!(progress, Progress::Key) {
        OBSERVE_SOURCE.with(|slot| {
            if let Some((source, observed)) = slot.borrow_mut().take() {
                observed.set(source.upgrade().is_some());
            }
        });
    }
    let stage = match progress {
        Progress::Name => 0,
        Progress::Key => 1,
        Progress::Entries => 2,
        Progress::Slot => 3,
        Progress::Move => 4,
        Progress::Reverse => 5,
    };
    CANCEL_EXPAND.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|(_, selected)| *selected == stage)
        {
            if let Some((handle, _)) = slot.take() {
                handle.cancel();
            }
        }
    });
}
#[cfg(not(test))]
fn observe(_: Progress) {}

pub(super) fn expand(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let Some((function, alias)) = object_keys_projection(projection) else {
        return Ok(batches);
    };
    let output_name = alias.unwrap_or(function.name.as_str());
    let mut expanded = rows::Rows::new(controls)?;
    for batch in batches {
        for row in batch {
            controls.check_interruption()?;
            match row.get(output_name) {
                None | Some(Value::Null) => {}
                Some(Value::Json(serde_json::Value::Array(keys))) => {
                    for key in keys {
                        controls.check_interruption()?;
                        let Some(key) = key.as_str() else {
                            return Err(QueryError::General(
                                "json_object_keys produced a non-text key".into(),
                            ));
                        };
                        let prepared = row::prepare(output_name, key, &row, controls)?;
                        expanded.push(prepared, controls)?;
                        observe(Progress::Slot);
                        controls.check_interruption()?;
                    }
                }
                Some(_) => {
                    return Err(QueryError::General(
                        "json_object_keys projection must produce text rows".into(),
                    ));
                }
            }
        }
    }
    let (rows, reservation) = expanded.into_parts(controls)?;
    let rows = output::retain(rows, reservation, controls)?;
    Ok(chunk_rows(rows, crate::executor::batch::DEFAULT_BATCH_SIZE))
}

#[cfg(test)]
pub(super) fn retain_output_for_test(
    rows: Vec<BatchRow>,
    reservation: crate::runtime::QueryMemoryReservation,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, crate::app::CassieError> {
    output::retain(rows, reservation, controls)
}

pub(super) fn requires_owner(projection: &[SelectItem]) -> bool {
    object_keys_projection(projection).is_some()
}

fn object_keys_projection(
    projection: &[SelectItem],
) -> Option<(&crate::sql::ast::FunctionCall, Option<&str>)> {
    let [item] = projection else { return None };
    let SelectItem::Function { function, alias } = item else {
        return None;
    };
    matches!(
        function.name.to_ascii_lowercase().as_str(),
        "json_object_keys" | "jsonb_object_keys"
    )
    .then_some((function, alias.as_deref()))
}
