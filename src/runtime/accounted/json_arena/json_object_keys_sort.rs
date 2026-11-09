//! Admitted stable lexical sorting of borrowed serde object keys.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "json_object_keys_sort_tests.rs"]
mod tests;
use std::cmp::Ordering;

#[cfg(test)]
thread_local! {
    static CANCEL_COMPARE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static CANCEL_MOVE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) fn cancel_object_keys_sort_compare_once(
    handle: crate::runtime::QueryCancellationHandle,
) {
    CANCEL_COMPARE.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
pub(crate) fn cancel_object_keys_sort_move_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_MOVE.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
fn compared() {
    CANCEL_COMPARE.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn compared() {}
#[cfg(test)]
fn moved() {
    CANCEL_MOVE.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn moved() {}

pub(super) struct Keys<'a> {
    pub(super) values: Vec<&'a str>,
    _memory: QueryMemoryReservation,
}
fn compare(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
) -> Result<Ordering, CassieError> {
    for (left, right) in left
        .as_bytes()
        .chunks(1024)
        .zip(right.as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        let order = left.cmp(right);
        compared();
        controls.check_interruption()?;
        if !order.is_eq() {
            return Ok(order);
        }
    }
    Ok(left.len().cmp(&right.len()))
}
pub(super) fn keys<'a>(
    map: &'a serde_json::Map<String, serde_json::Value>,
    controls: &QueryExecutionControls,
) -> Result<Keys<'a>, CassieError> {
    sort_iter(map.len(), map.keys().map(String::as_str), controls)
}

fn sort_iter<'a>(
    length: usize,
    keys: impl IntoIterator<Item = &'a str>,
    controls: &QueryExecutionControls,
) -> Result<Keys<'a>, CassieError> {
    let empty = controls.reserve_query_memory(0)?;
    let (first, first_memory) = crate::runtime::owned_memory::try_reserve_vec(length, &empty)?;
    let mut first = Keys {
        values: first,
        _memory: first_memory,
    };
    let (second, second_memory) = crate::runtime::owned_memory::try_reserve_vec(length, &empty)?;
    let mut second = Keys {
        values: second,
        _memory: second_memory,
    };
    for key in keys {
        controls.check_interruption()?;
        if first.values.len() >= length {
            return Err(CassieError::Execution(
                "JSON object key count changed during sorting".into(),
            ));
        }
        first.values.push(key);
    }
    if first.values.len() != length {
        return Err(CassieError::Execution(
            "JSON object key count changed during sorting".into(),
        ));
    }
    let mut width = 1_usize;
    while width < first.values.len() {
        second.values.clear();
        let mut start = 0;
        while start < first.values.len() {
            let middle = start.saturating_add(width).min(first.values.len());
            let end = middle.saturating_add(width).min(first.values.len());
            let (mut left, mut right) = (start, middle);
            while left < middle || right < end {
                controls.check_interruption()?;
                let take_left = right == end
                    || (left < middle
                        && compare(first.values[left], first.values[right], controls)?
                            != Ordering::Greater);
                if take_left {
                    second.values.push(first.values[left]);
                    left += 1;
                } else {
                    second.values.push(first.values[right]);
                    right += 1;
                }
                moved();
                controls.check_interruption()?;
            }
            start = end;
        }
        std::mem::swap(&mut first, &mut second);
        width = width.saturating_mul(2);
    }
    drop(second);
    drop(empty);
    Ok(first)
}
