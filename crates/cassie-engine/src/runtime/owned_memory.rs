//! Capacity-coupled allocation helpers for retained query results and frontend owners.

use cassie_core::request::accounted::allocation;

use crate::app::CassieError;

use super::QueryMemoryReservation;

/// Reserves a fresh directory before allocation. The returned lease follows the backing buffer.
///
/// # Errors
///
/// Returns a resource-limit error if the size, reservation, or allocation cannot be satisfied.
#[doc(hidden)]
pub fn try_reserve_vec<T>(
    length: usize,
    admission: &QueryMemoryReservation,
) -> Result<(Vec<T>, QueryMemoryReservation), CassieError> {
    let (values, reservation) = allocation::try_reserve_vec(length, admission.as_core())?;
    Ok((values, QueryMemoryReservation::from_core(reservation)))
}

/// Admits the complete new directory while the old allocation remains owned.
///
/// # Errors
///
/// Returns a resource-limit error if the size, reservation, or allocation cannot be satisfied.
#[doc(hidden)]
pub fn grow_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    reservation: &mut QueryMemoryReservation,
) -> Result<(), CassieError> {
    Ok(allocation::grow_vec(
        values,
        additional,
        reservation.as_core_mut(),
    )?)
}

#[doc(hidden)]
pub fn allocation_error(error: &std::collections::TryReserveError) -> CassieError {
    CassieError::ResourceLimit(format!("unable to allocate admitted query result: {error}"))
}
