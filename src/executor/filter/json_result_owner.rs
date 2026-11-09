//! Retain an existing JSON construction workspace through its returned value.
use crate::app::CassieError;
use crate::executor::filter::EvaluationMemory;
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER_OUTPUT: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn cancel_after_output_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_AFTER_OUTPUT.with(|slot| *slot.borrow_mut() = Some(handle));
}

struct PendingOutput {
    value: Value,
    memory: Option<QueryMemoryReservation>,
}

pub(super) fn retain_output(
    value: Value,
    memory: Option<QueryMemoryReservation>,
    owner: Option<&EvaluationMemory>,
    controls: Option<&QueryExecutionControls>,
    mismatch: &'static str,
) -> Result<Value, QueryError> {
    let mut pending = PendingOutput { value, memory };
    if let (Value::String(text), Some(owner), Some(memory), Some(controls)) =
        (&pending.value, owner, pending.memory.as_mut(), controls)
    {
        if !owner.belongs_to(controls) || !memory.belongs_to(controls) {
            return Err(CassieError::ResourceLimit(mismatch.into()).into());
        }
        if text.capacity() > memory.bytes() {
            memory.try_grow(text.capacity() - memory.bytes())?;
        }
        check_transfer(controls)?;
        if let Some(memory) = pending.memory.take() {
            owner.adopt(memory);
        }
    }
    Ok(pending.value)
}

pub(super) fn retain_json_tree(
    value: Value,
    memory: Option<QueryMemoryReservation>,
    owner: Option<&EvaluationMemory>,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let mut pending = PendingOutput { value, memory };
    if let (Value::Json(_), Some(owner), Some(memory), Some(controls)) =
        (&pending.value, owner, pending.memory.as_ref(), controls)
    {
        if !owner.belongs_to(controls) || !memory.belongs_to(controls) {
            return Err(CassieError::ResourceLimit(
                "JSONPath result owner uses a different query tracker".into(),
            )
            .into());
        }
        check_transfer(controls)?;
        if let Some(memory) = pending.memory.take() {
            owner.adopt(memory);
        }
    }
    Ok(pending.value)
}

fn check_transfer(controls: &QueryExecutionControls) -> Result<(), QueryError> {
    #[cfg(test)]
    CANCEL_AFTER_OUTPUT.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
    controls.check_interruption().map_err(Into::into)
}
