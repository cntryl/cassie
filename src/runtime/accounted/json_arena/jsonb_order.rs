//! Admitted, interruptible JSONB object-key ordering.
use std::cmp::Ordering;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{NodeId, ObjectIndex};

pub(super) struct Order {
    indices: Vec<usize>,
    _memory: QueryMemoryReservation,
}

impl Order {
    pub(super) fn get(&self, position: usize) -> Option<usize> {
        self.indices.get(position).copied()
    }
}

pub(super) fn sorted(
    index: &ObjectIndex<NodeId>,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(),
) -> Result<Order, CassieError> {
    let empty_memory = controls.reserve_query_memory(0)?;
    let first_memory;
    let mut first;
    (first, first_memory) =
        crate::runtime::owned_memory::try_reserve_vec(index.values_len(), &empty_memory)?;
    let mut next = index.first_index()?;
    while let Some(position) = next {
        controls.check_interruption()?;
        first.push(position);
        next = index.next_index(position, &mut || {})?;
    }
    let second_memory;
    let mut second;
    (second, second_memory) =
        crate::runtime::owned_memory::try_reserve_vec(first.len(), &empty_memory)?;
    let mut width = 1_usize;
    while width < first.len() {
        second.clear();
        let mut start = 0;
        while start < first.len() {
            let middle = start.saturating_add(width).min(first.len());
            let end = middle.saturating_add(width).min(first.len());
            let (mut left, mut right) = (start, middle);
            while left < middle || right < end {
                controls.check_interruption()?;
                let take_left = right == end
                    || (left < middle
                        && compare(
                            index.item(first[left]).0,
                            index.item(first[right]).0,
                            controls,
                            observer,
                        )? != Ordering::Greater);
                if take_left {
                    second.push(first[left]);
                    left += 1;
                } else {
                    second.push(first[right]);
                    right += 1;
                }
            }
            start = end;
        }
        std::mem::swap(&mut first, &mut second);
        width = width.saturating_mul(2);
    }
    drop(second);
    drop(second_memory);
    drop(empty_memory);
    Ok(Order {
        indices: first,
        _memory: first_memory,
    })
}

fn compare(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(),
) -> Result<Ordering, CassieError> {
    let order = left.len().cmp(&right.len());
    if order != Ordering::Equal {
        return Ok(order);
    }
    for (a, b) in left
        .as_bytes()
        .chunks(1024)
        .zip(right.as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        let result = a.cmp(b);
        observer();
        controls.check_interruption()?;
        if result != Ordering::Equal {
            return Ok(result);
        }
    }
    Ok(Ordering::Equal)
}
