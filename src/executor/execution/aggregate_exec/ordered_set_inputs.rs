//! Value-first storage for mode and hypothetical-set working inputs.

use crate::runtime::QueryMemoryReservation;

pub(super) struct RetainedInputs<T> {
    pub(super) values: Vec<T>,
    pub(super) retained: QueryMemoryReservation,
    pub(super) capacity: QueryMemoryReservation,
}
