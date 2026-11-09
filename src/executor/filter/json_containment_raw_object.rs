//! Checked root object containment; the caller retains both input strings.
use super::{object, render::RenderError, scan};
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;
use std::cmp::Ordering;

#[cfg(test)]
thread_local! {
    static CANCEL_COMPARE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn cancel_sort_once(handle: crate::runtime::QueryCancellationHandle) {
    object::cancel_lexical_sort_once(handle);
}
#[cfg(test)]
pub(super) fn cancel_compare_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_COMPARE.with(|slot| *slot.borrow_mut() = Some(handle));
}

pub(super) fn evaluate(
    candidate: &str,
    required: &str,
    controls: &QueryExecutionControls,
    after_scalar: &mut impl FnMut(bool),
) -> Result<Option<bool>, QueryError> {
    if !is_object(candidate, controls)? || !is_object(required, controls)? {
        return Ok(None);
    }
    let candidate_span = scan(candidate, controls)
        .map_err(RenderError::from)
        .map_err(RenderError::into_query_error)?;
    let required_span = scan(required, controls)
        .map_err(RenderError::from)
        .map_err(RenderError::into_query_error)?;
    let candidates = object::parse(&candidate[candidate_span], controls)
        .map_err(RenderError::from)
        .map_err(RenderError::into_query_error)?;
    let requirements = object::parse(&required[required_span], controls)
        .map_err(RenderError::from)
        .map_err(RenderError::into_query_error)?;
    let candidates = candidates.into_lexical(controls)?;
    let requirements = requirements.into_lexical(controls)?;
    let (mut left, mut right) = (0, 0);
    while let Some((key, value)) = requirements.get(right) {
        controls.check_interruption()?;
        let Some((candidate_key, candidate_value)) = candidates.get(left) else {
            return Ok(Some(false));
        };
        match compare(candidate_key, key, controls)? {
            Ordering::Less => left += 1,
            Ordering::Greater => return Ok(Some(false)),
            Ordering::Equal => {
                if !super::super::super::json_containment::evaluate_nested(
                    candidate_value,
                    value,
                    controls,
                    after_scalar,
                )? {
                    return Ok(Some(false));
                }
                left += 1;
                right += 1;
            }
        }
    }
    Ok(Some(true))
}

fn is_object(input: &str, controls: &QueryExecutionControls) -> Result<bool, QueryError> {
    for chunk in input.as_bytes().chunks(1024) {
        controls.check_interruption()?;
        for byte in chunk {
            if !matches!(*byte, b' ' | b'\n' | b'\r' | b'\t') {
                return Ok(*byte == b'{');
            }
        }
    }
    Ok(false)
}

pub(super) fn compare(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
) -> Result<Ordering, QueryError> {
    for (left, right) in left
        .as_bytes()
        .chunks(1024)
        .zip(right.as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        let ordering = left.cmp(right);
        #[cfg(test)]
        CANCEL_COMPARE.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.cancel();
            }
        });
        controls.check_interruption()?;
        if ordering != Ordering::Equal {
            return Ok(ordering);
        }
    }
    Ok(left.len().cmp(&right.len()))
}
