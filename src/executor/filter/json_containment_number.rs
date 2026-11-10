//! Checked equality of validated finite JSON number tokens.
use crate::executor::QueryError;
use crate::runtime::accounted::{json_arena::raw_number, Accounted};
use crate::runtime::QueryExecutionControls;

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER_FORMAT: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static CANCEL_AFTER_COMPARE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn cancel_after_number_format_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_AFTER_FORMAT.with(|slot| *slot.borrow_mut() = Some(handle));
}

#[cfg(test)]
pub(crate) fn cancel_after_number_compare_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_AFTER_COMPARE.with(|slot| *slot.borrow_mut() = Some(handle));
}

pub(super) fn equal(
    candidate: &str,
    required: &str,
    controls: &QueryExecutionControls,
) -> Result<bool, QueryError> {
    let candidate = format(candidate, controls)?;
    let required = format(required, controls)?;
    let left = normalized(candidate.get().as_bytes(), controls)?;
    let right = normalized(required.get().as_bytes(), controls)?;
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.chunks(1024).zip(right.chunks(1024)) {
        controls.check_interruption()?;
        let equal = left == right;
        #[cfg(test)]
        CANCEL_AFTER_COMPARE.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.cancel();
            }
        });
        controls.check_interruption()?;
        if !equal {
            return Ok(false);
        }
    }
    Ok(true)
}

fn format(token: &str, controls: &QueryExecutionControls) -> Result<Accounted<String>, QueryError> {
    #[cfg(not(test))]
    let result = raw_number::format(token, controls);
    #[cfg(test)]
    let result = raw_number::format_observing(token, controls, || {
        CANCEL_AFTER_FORMAT.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.cancel();
            }
        });
    });
    result.map_err(|error| match error {
        raw_number::RawNumberError::Overflow => {
            QueryError::General("value overflows numeric format".into())
        }
        raw_number::RawNumberError::Control(error) => error.into(),
    })
}

fn normalized<'a>(
    text: &'a [u8],
    controls: &QueryExecutionControls,
) -> Result<&'a [u8], QueryError> {
    let mut dot = None;
    for (index, chunk) in text.chunks(1024).enumerate() {
        controls.check_interruption()?;
        if let Some(offset) = chunk.iter().position(|byte| *byte == b'.') {
            dot = Some(index * 1024 + offset);
            break;
        }
    }
    let Some(dot) = dot else {
        return Ok(text);
    };
    let mut end = text.len();
    while end > dot + 1 {
        controls.check_interruption()?;
        let start = end.saturating_sub(1024).max(dot + 1);
        let chunk = &text[start..end];
        let zeros = chunk.iter().rev().take_while(|byte| **byte == b'0').count();
        end -= zeros;
        if zeros < chunk.len() {
            break;
        }
    }
    if end == dot + 1 {
        end = dot;
    }
    Ok(&text[..end])
}
