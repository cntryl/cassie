//! Stable, admitted and interruptible permutation for window sort keys.
use std::cmp::Ordering;

use super::{AdmittedWindowBuffer, QueryError, QueryExecutionControls};

#[cfg(test)]
#[path = "stable_order_tests.rs"]
mod tests;

pub(super) fn order_by(
    count: usize,
    controls: &QueryExecutionControls,
    mut compare: impl FnMut(usize, usize) -> Result<Ordering, QueryError>,
) -> Result<AdmittedWindowBuffer<usize>, QueryError> {
    let mut order = AdmittedWindowBuffer::new(count, controls)?;
    let mut scratch = AdmittedWindowBuffer::new(count, controls)?;
    for index in 0..count {
        controls.check_interruption()?;
        order.values.push(index);
        scratch.values.push(0);
    }
    let mut width = 1_usize;
    while width < count {
        let mut start = 0;
        while start < count {
            controls.check_interruption()?;
            let middle = start.saturating_add(width).min(count);
            let end = middle.saturating_add(width).min(count);
            let (mut left, mut right, mut output) = (start, middle, start);
            while left < middle && right < end {
                controls.check_interruption()?;
                let ordering = compare(order.values[left], order.values[right])?;
                controls.check_interruption()?;
                if ordering == Ordering::Greater {
                    scratch.values[output] = order.values[right];
                    right += 1;
                } else {
                    scratch.values[output] = order.values[left];
                    left += 1;
                }
                output += 1;
            }
            while left < middle {
                controls.check_interruption()?;
                scratch.values[output] = order.values[left];
                left += 1;
                output += 1;
            }
            while right < end {
                controls.check_interruption()?;
                scratch.values[output] = order.values[right];
                right += 1;
                output += 1;
            }
            start = end;
        }
        std::mem::swap(&mut order, &mut scratch);
        width = width.saturating_mul(2);
    }
    controls.check_interruption()?;
    Ok(order)
}
