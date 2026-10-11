//! Capacity-coupled allocation helpers for retained query results and frontend owners.

use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};

use super::QueryMemoryReservation;

/// Reserves a fresh directory before allocation. The returned lease follows the backing buffer.
pub(crate) fn try_reserve_vec<T>(
    length: usize,
    admission: &QueryMemoryReservation,
) -> Result<(Vec<T>, QueryMemoryReservation), CassieError> {
    let reservation = admission.try_sibling(mul(length, size_of::<T>())?)?;
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(allocation_error)?;
    if values.capacity() > length && size_of::<T>() != 0 {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted query directory capacity".to_owned(),
        ));
    }
    Ok((values, reservation))
}

/// Admits the complete new directory while the old allocation remains owned.
pub(crate) fn grow_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    reservation: &mut QueryMemoryReservation,
) -> Result<(), CassieError> {
    let needed = add(values.len(), additional)?;
    if needed <= values.capacity() {
        return Ok(());
    }
    let capacity = needed.max(4).checked_next_power_of_two().ok_or_else(|| {
        CassieError::ResourceLimit("query directory capacity overflow".to_owned())
    })?;
    let old_bytes = mul(values.capacity(), size_of::<T>())?;
    let (mut replacement, replacement_memory) = try_reserve_vec(capacity, reservation)?;
    replacement.append(values);
    let old = std::mem::replace(values, replacement);
    drop(old);
    reservation.shrink_to(reservation.bytes() - old_bytes);
    reservation.absorb(replacement_memory);
    Ok(())
}

pub(crate) fn allocation_error(error: std::collections::TryReserveError) -> CassieError {
    CassieError::ResourceLimit(format!("unable to allocate admitted query result: {error}"))
}
