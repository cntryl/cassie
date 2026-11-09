//! Controlled compact serialization for JSON scalar column/parameter leaves.
use super::{scalar_from_value, EvalContext, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::runtime::accounted::compact_json;
#[cfg(test)]
use crate::runtime::accounted::compact_json::Progress;
use crate::types::Value;

#[cfg(test)]
#[path = "json_scalar_leaf_tests.rs"]
mod tests;

#[cfg(test)]
thread_local! {
    static CANCEL_COPY: std::cell::RefCell<Option<(crate::runtime::QueryCancellationHandle, usize)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn cancel_copy_once(handle: crate::runtime::QueryCancellationHandle, number: usize) {
    CANCEL_COPY.with(|slot| *slot.borrow_mut() = Some((handle, number)));
}

#[cfg(test)]
pub(crate) fn clear_copy_cancellation() {
    CANCEL_COPY.with(|slot| *slot.borrow_mut() = None);
}

pub(super) fn from_value(
    value: &Value,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let controls = context.controls.or_else(|| {
        context
            .session
            .and_then(crate::app::CassieSession::statement_execution_controls)
    });
    let (Value::Json(json), None, Some(owner), Some(controls)) =
        (value, context.memory, context.deletion_memory, controls)
    else {
        return Ok(scalar_from_value(value));
    };
    if !owner.belongs_to(controls) {
        return Err(CassieError::ResourceLimit(
            "JSON scalar owner uses a different query tracker".into(),
        )
        .into());
    }
    #[cfg(not(test))]
    let output = compact_json::serialize(json, controls)?;
    #[cfg(test)]
    let output = compact_json::serialize_observing(json, controls, progress)?;
    let (text, memory) = output.into_parts();
    if !memory.belongs_to(controls) {
        drop(text);
        drop(memory);
        return Err(CassieError::ResourceLimit(
            "JSON scalar output uses a different query tracker".into(),
        )
        .into());
    }
    owner.adopt(memory);
    Ok(ScalarValue::Json(text))
}

#[cfg(test)]
fn progress(progress: Progress) {
    if matches!(progress, Progress::Copy) {
        CANCEL_COPY.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some((handle, remaining)) = slot.as_mut() {
                *remaining -= 1;
                if *remaining == 0 {
                    handle.cancel();
                    *slot = None;
                }
            }
        });
    }
}
