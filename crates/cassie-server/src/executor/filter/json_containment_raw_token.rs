//! Checked raw-token copies covered by the caller's existing pair reservation.
#[cfg(test)]
use super::Progress;
use super::{render::RenderError, scan_observing};
use crate::app::CassieError;
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;

#[cfg(test)]
thread_local! {
    static CANCEL_SCAN: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static CANCEL_COPY: std::cell::RefCell<Option<(crate::runtime::QueryCancellationHandle, usize)>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(in crate::executor::filter) fn cancel_scan_once(
    handle: crate::runtime::QueryCancellationHandle,
) {
    CANCEL_SCAN.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
pub(in crate::executor::filter) fn cancel_copy_once(
    handle: crate::runtime::QueryCancellationHandle,
    number: usize,
) {
    CANCEL_COPY.with(|slot| *slot.borrow_mut() = Some((handle, number)));
}

pub(in crate::executor::filter) fn copy(
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<String, QueryError> {
    let span = scan_observing(input, controls, |progress| {
        #[cfg(test)]
        if matches!(progress, Progress::Input) {
            CANCEL_SCAN.with(|slot| {
                if let Some(handle) = slot.borrow_mut().take() {
                    handle.cancel();
                }
            });
        }
        #[cfg(not(test))]
        let _ = progress;
    })
    .map_err(RenderError::from)
    .map_err(RenderError::into_query_error)?;
    let source = &input[span];
    let mut result = String::new();
    result
        .try_reserve_exact(source.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if result.capacity() > source.len() {
        return Err(CassieError::ResourceLimit(
            "raw containment token exceeds its admission".into(),
        )
        .into());
    }
    let mut start = 0;
    while start < source.len() {
        controls.check_interruption()?;
        let mut end = start.saturating_add(1024).min(source.len());
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        result.push_str(&source[start..end]);
        #[cfg(test)]
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
        controls.check_interruption()?;
        start = end;
    }
    Ok(result)
}
