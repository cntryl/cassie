//! Controlled top-level raw concatenation. The caller retains both sources.
use std::cmp::Ordering;

use crate::runtime::accounted::compact_json::{OwnedJsonText, Sink};
use crate::runtime::QueryExecutionControls;

use super::{object, render, scan};
use render::RenderError;

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER_COPY: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn cancel_after_copy_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_AFTER_COPY.with(|slot| *slot.borrow_mut() = Some(handle));
}

pub(super) fn concatenate<'a>(
    left: &'a str,
    right: &'a str,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    let left_span = scan(left, controls)?;
    let right_span = scan(right, controls)?;
    let left_value = &left[left_span];
    let right_value = &right[right_span];
    let mut output = Sink::for_arena(controls)?;
    if left_value.starts_with('{') && right_value.starts_with('{') {
        // Decode both sets of keys before ordering or formatting any survivor.
        let left_fields = object::parse(left, controls)?;
        let right_fields = object::parse(right, controls)?;
        let left_fields = left_fields.into_ordered(controls)?;
        let right_fields = right_fields.into_ordered(controls)?;
        output.write("{", &mut |_| {})?;
        let (mut l, mut r, mut emitted) = (0, 0, false);
        loop {
            controls.check_interruption()?;
            let a = left_fields.get(l);
            let b = right_fields.get(r);
            let selected = match (a, b) {
                (None, None) => break,
                (Some(value), None) => {
                    l += 1;
                    value
                }
                (None, Some(value)) => {
                    r += 1;
                    value
                }
                (Some(a), Some(b)) => match compare(a.0, b.0, controls)? {
                    Ordering::Less => {
                        l += 1;
                        a
                    }
                    Ordering::Greater => {
                        r += 1;
                        b
                    }
                    Ordering::Equal => {
                        l += 1;
                        r += 1;
                        b
                    }
                },
            };
            if emitted {
                output.write(", ", &mut |_| {})?;
            }
            output.quoted(selected.0, &mut |_| {})?;
            output.write(": ", &mut |_| {})?;
            append(&mut output, selected.1, controls)?;
            emitted = true;
        }
        output.write("}", &mut |_| {})?;
    } else {
        output.write("[", &mut |_| {})?;
        let mut emitted = false;
        for value in [left_value, right_value] {
            if value.starts_with('[') {
                let mut scanner = render::array_scanner(value, controls)?;
                let _ = scanner.next()?;
                loop {
                    scanner.whitespace()?;
                    if scanner.peek() == Some(b']') {
                        let _ = scanner.next()?;
                        break;
                    }
                    if scanner.peek() == Some(b',') {
                        let _ = scanner.next()?;
                        scanner.whitespace()?;
                    }
                    let span = scanner.value_span()?;
                    if emitted {
                        output.write(", ", &mut |_| {})?;
                    }
                    append(&mut output, &value[span], controls)?;
                    emitted = true;
                }
            } else {
                if emitted {
                    output.write(", ", &mut |_| {})?;
                }
                append(&mut output, value, controls)?;
                emitted = true;
            }
        }
        output.write("]", &mut |_| {})?;
    }
    Ok(output.finish_output())
}

fn append<'a>(
    output: &mut Sink<'a>,
    value: &'a str,
    controls: &'a QueryExecutionControls,
) -> Result<(), RenderError> {
    let formatted = render::render_value(value, controls)?;
    output.write(formatted.as_str(), &mut |_| {})?;
    #[cfg(test)]
    CANCEL_AFTER_COPY.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
    controls.check_interruption()?;
    Ok(())
}

fn compare(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
) -> Result<Ordering, RenderError> {
    let length = left.len().cmp(&right.len());
    if length != Ordering::Equal {
        return Ok(length);
    }
    for (a, b) in left
        .as_bytes()
        .chunks(1024)
        .zip(right.as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        let order = a.cmp(b);
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(Ordering::Equal)
}
