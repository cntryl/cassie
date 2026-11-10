//! Checked comparison of visited JSON values in borrowed native arrays.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::types::{SqlArray, Value};
use std::cmp::Ordering;

#[cfg(test)]
thread_local! {
    static CANCEL_RIGHT_COPY: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn cancel_right_copy_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_RIGHT_COPY.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
fn right_progress(progress: crate::runtime::accounted::compact_json::Progress) {
    if matches!(
        progress,
        crate::runtime::accounted::compact_json::Progress::Copy
    ) {
        CANCEL_RIGHT_COPY.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.cancel();
            }
        });
    }
}
#[cfg(not(test))]
fn right_progress(_: crate::runtime::accounted::compact_json::Progress) {}

pub(in crate::executor::filter) fn compare_arrays_controlled(
    left: &SqlArray,
    right: &SqlArray,
    controls: &QueryExecutionControls,
) -> Result<Option<Ordering>, CassieError> {
    for (left, right) in left.elements().iter().zip(right.elements()) {
        controls.check_interruption()?;
        let order = match (left, right) {
            (Value::Json(left), Value::Json(right)) => {
                let left = crate::runtime::accounted::compact_json::serialize(left, controls)?;
                let right = crate::runtime::accounted::compact_json::serialize_observing(
                    right,
                    controls,
                    right_progress,
                )?;
                super::super::json_order::compare_controlled(
                    left.as_str(),
                    right.as_str(),
                    controls,
                )?
                .or_else(|| super::super::json_order::compare(left.as_str(), right.as_str()))
            }
            _ => super::compare_elements(left, right),
        };
        let Some(order) = order else { return Ok(None) };
        if !order.is_eq() {
            return Ok(Some(order));
        }
    }
    let order = left.len().cmp(&right.len());
    if !order.is_eq() {
        return Ok(Some(order));
    }
    let order = left.dimensions().len().cmp(&right.dimensions().len());
    if !order.is_eq() {
        return Ok(Some(order));
    }
    for (left, right) in left.dimensions().iter().zip(right.dimensions()) {
        controls.check_interruption()?;
        let order = left.length.cmp(&right.length);
        if !order.is_eq() {
            return Ok(Some(order));
        }
    }
    for (left, right) in left.dimensions().iter().zip(right.dimensions()) {
        controls.check_interruption()?;
        let order = left.lower_bound.cmp(&right.lower_bound);
        if !order.is_eq() {
            return Ok(Some(order));
        }
    }
    Ok(Some(Ordering::Equal))
}
