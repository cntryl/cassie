//! Stable, admitted ordering of reached object keys, with last duplicate wins.
use std::cmp::Ordering;
use std::marker::PhantomData;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::Fields;

pub(super) struct Order<'a> {
    indices: Vec<usize>,
    _source: PhantomData<&'a ()>,
    _memory: QueryMemoryReservation,
}

impl Order<'_> {
    #[cfg(test)]
    pub(super) fn indices(&self) -> &[usize] {
        &self.indices
    }

    pub(super) fn into_parts(self) -> (Vec<usize>, QueryMemoryReservation) {
        (self.indices, self._memory)
    }
}

pub(super) fn sort<'a>(
    fields: &'a Fields<'_>,
    controls: &QueryExecutionControls,
) -> Result<Order<'a>, CassieError> {
    sort_inner(fields, controls, || {})
}

#[cfg(test)]
pub(super) fn sort_observing<'a>(
    fields: &'a Fields<'_>,
    controls: &QueryExecutionControls,
    observe: impl FnMut(),
) -> Result<Order<'a>, CassieError> {
    sort_inner(fields, controls, observe)
}

fn sort_inner<'a>(
    fields: &'a Fields<'_>,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(),
) -> Result<Order<'a>, CassieError> {
    if !fields.memory.belongs_to(controls) {
        return Err(CassieError::ResourceLimit(
            "JSON object owner uses a different query tracker".into(),
        ));
    }
    let first_memory;
    let mut first;
    (first, first_memory) =
        crate::runtime::owned_memory::try_reserve_vec(fields.entries.len(), &fields.memory)?;
    let second_memory;
    let mut second;
    (second, second_memory) =
        crate::runtime::owned_memory::try_reserve_vec(fields.entries.len(), &fields.memory)?;
    for index in 0..fields.entries.len() {
        controls.check_interruption()?;
        first.push(index);
    }
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
                let choose_left = right == end
                    || (left < middle
                        && compare(
                            fields.entries[first[left]].key.get(),
                            fields.entries[first[right]].key.get(),
                            controls,
                            &mut observe,
                        )? != Ordering::Greater);
                if choose_left {
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
    second.clear();
    for index in 0..first.len() {
        controls.check_interruption()?;
        let current = first[index];
        let last = index + 1 == first.len()
            || compare(
                fields.entries[current].key.get(),
                fields.entries[first[index + 1]].key.get(),
                controls,
                &mut observe,
            )? != Ordering::Equal;
        if last {
            second.push(current);
        }
    }
    drop(first);
    drop(first_memory);
    Ok(Order {
        indices: second,
        _source: PhantomData,
        _memory: second_memory,
    })
}

fn compare(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
    observe: &mut impl FnMut(),
) -> Result<Ordering, CassieError> {
    let length = left.len().cmp(&right.len());
    if length != Ordering::Equal {
        return Ok(length);
    }
    for (left, right) in left
        .as_bytes()
        .chunks(1024)
        .zip(right.as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        let cmp = left.cmp(right);
        observe();
        controls.check_interruption()?;
        if cmp != Ordering::Equal {
            return Ok(cmp);
        }
    }
    Ok(Ordering::Equal)
}
