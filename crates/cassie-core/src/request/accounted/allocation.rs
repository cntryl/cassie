//! Capacity-coupled allocation helpers for retained query results and frontend owners.

use std::mem::size_of;

use super::retained::{add, mul};
use super::{AccountingError, RequestMemoryReservation};

/// Reserves a fresh directory before allocation. The returned lease follows the backing buffer.
///
/// # Errors
///
/// Returns an accounting error if the size, reservation, or allocation cannot be satisfied.
#[doc(hidden)]
pub fn try_reserve_vec<T>(
    length: usize,
    admission: &RequestMemoryReservation,
) -> Result<(Vec<T>, RequestMemoryReservation), AccountingError> {
    let reservation = admission.try_sibling(mul(length, size_of::<T>())?)?;
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(allocation_error)?;
    if values.capacity() > length && size_of::<T>() != 0 {
        return Err(AccountingError::AllocatorCapacityExceeded);
    }
    Ok((values, reservation))
}

/// Admits the complete new directory while the old allocation remains owned.
///
/// # Errors
///
/// Returns an accounting error if the size, reservation, or allocation cannot be satisfied.
#[doc(hidden)]
pub fn grow_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    reservation: &mut RequestMemoryReservation,
) -> Result<(), AccountingError> {
    let needed = add(values.len(), additional)?;
    if needed <= values.capacity() {
        return Ok(());
    }
    let capacity = needed
        .max(4)
        .checked_next_power_of_two()
        .ok_or(AccountingError::DirectoryCapacityOverflow)?;
    let old_bytes = mul(values.capacity(), size_of::<T>())?;
    let (mut replacement, replacement_memory) = try_reserve_vec(capacity, reservation)?;
    replacement.append(values);
    let old = std::mem::replace(values, replacement);
    drop(old);
    reservation.shrink_to(reservation.bytes() - old_bytes);
    reservation.absorb(replacement_memory);
    Ok(())
}

#[doc(hidden)]
#[must_use]
pub fn allocation_error(error: std::collections::TryReserveError) -> AccountingError {
    AccountingError::Allocation(error)
}
