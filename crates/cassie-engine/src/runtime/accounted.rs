use cassie_core::request::accounted::{
    Accounted as CoreAccounted, AccountedVec as CoreAccountedVec, AccountingError,
};

use crate::app::CassieError;

use super::{QueryExecutionControls, QueryMemoryReservation};

#[path = "accounted/compact_json.rs"]
pub(crate) mod compact_json;
#[path = "accounted/json.rs"]
#[doc(hidden)]
pub mod json;

#[path = "accounted/json_arena.rs"]
#[doc(hidden)]
pub mod json_arena;

impl From<AccountingError> for CassieError {
    fn from(error: AccountingError) -> Self {
        Self::ResourceLimit(error.to_string())
    }
}

/// A retained value paired with the query-memory reservation that owns its budget.
#[doc(hidden)]
#[derive(Debug)]
#[repr(transparent)]
pub struct Accounted<T>(CoreAccounted<T>);

impl<T> Accounted<T> {
    pub(crate) fn from_admitted(value: T, reservation: QueryMemoryReservation) -> Self {
        Self(CoreAccounted::from_admitted(value, reservation.into_core()))
    }

    /// Reserves the complete retained-size estimate before constructing the value.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error without invoking `build` when the query budget is too small.
    pub(crate) fn try_new(
        controls: &QueryExecutionControls,
        retained_bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<Self, CassieError> {
        Ok(Self(CoreAccounted::try_new(
            controls.request_memory(),
            retained_bytes,
            build,
        )?))
    }

    #[must_use]
    #[doc(hidden)]
    pub const fn get(&self) -> &T {
        self.0.get()
    }

    pub(crate) fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }

    pub(crate) fn shares_tracker(&self, controls: &QueryExecutionControls) -> bool {
        self.0.belongs_to(controls.request_memory())
    }

    #[must_use]
    pub(crate) const fn accounted_bytes(&self) -> usize {
        self.0.accounted_bytes()
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> (T, QueryMemoryReservation) {
        let (value, reservation) = self.0.into_parts();
        (value, QueryMemoryReservation::from_core(reservation))
    }
}

/// A retained vector and its growable reservation on the shared request tracker.
#[derive(Debug)]
#[repr(transparent)]
pub(crate) struct AccountedVec<T>(CoreAccountedVec<T>);

impl<T> AccountedVec<T> {
    pub(crate) fn try_new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self(CoreAccountedVec::try_new(controls.request_memory())?))
    }

    pub(crate) fn try_push_with(
        &mut self,
        variable_bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<(), CassieError> {
        Ok(self.0.try_push_with(variable_bytes, build)?)
    }

    pub(crate) fn try_push_with_result(
        &mut self,
        variable_bytes: usize,
        build: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<(), CassieError> {
        self.0.try_push_with_result(variable_bytes, build)
    }

    pub(crate) fn try_push_clone(
        &mut self,
        value: &T,
        variable_bytes: usize,
    ) -> Result<(), CassieError>
    where
        T: Clone,
    {
        Ok(self.0.try_push_clone(value, variable_bytes)?)
    }

    #[must_use]
    pub(crate) fn as_slice(&self) -> &[T] {
        self.0.as_slice()
    }

    #[must_use]
    pub(crate) const fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub(crate) const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub(crate) const fn accounted_bytes(&self) -> usize {
        self.0.accounted_bytes()
    }

    #[must_use]
    pub(crate) fn into_parts(self) -> (Vec<T>, QueryMemoryReservation) {
        let (values, reservation) = self.0.into_parts();
        (values, QueryMemoryReservation::from_core(reservation))
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;

    use super::{Accounted, AccountedVec, CassieError, QueryExecutionControls};

    fn controls_with_budget(query_memory_budget_bytes: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    #[test]
    fn should_reserve_before_building_an_accounted_value() {
        // Arrange
        let controls = controls_with_budget(3);
        let build_calls = Cell::new(0);

        // Act
        let result = Accounted::try_new(&controls, 4, || build_calls.set(1));

        // Assert
        assert!(result.is_err());
        assert_eq!(build_calls.get(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_hold_an_accounted_value_reservation_until_drop() {
        // Arrange
        let controls = controls_with_budget(16);

        // Act
        let value =
            Accounted::try_new(&controls, 8, || String::from("cassie")).expect("accounted value");

        // Assert
        assert_eq!(value.get(), "cassie");
        assert_eq!(value.accounted_bytes(), 8);
        assert_eq!(controls.current_query_memory_bytes(), 8);
        drop(value);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_reserve_before_cloning_into_an_accounted_vector() {
        // Arrange
        #[derive(Debug)]
        struct CloneProbe(Rc<Cell<usize>>);

        impl Clone for CloneProbe {
            fn clone(&self) -> Self {
                self.0.set(self.0.get() + 1);
                Self(Rc::clone(&self.0))
            }
        }

        let clone_calls = Rc::new(Cell::new(0));
        let value = CloneProbe(Rc::clone(&clone_calls));
        let controls = controls_with_budget(size_of::<CloneProbe>() - 1);
        let mut values = AccountedVec::try_new(&controls).expect("empty accounted vector");

        // Act
        let result = values.try_push_clone(&value, 0);

        // Assert
        assert!(result.is_err());
        assert_eq!(clone_calls.get(), 0);
        assert!(values.is_empty());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_track_accounted_vector_reservation_lifecycle() {
        // Arrange
        let controls = controls_with_budget(128);
        let mut values = AccountedVec::try_new(&controls).expect("empty accounted vector");

        // Act
        values
            .try_push_with(3, || String::from("one"))
            .expect("first retained value");
        values
            .try_push_with(3, || String::from("two"))
            .expect("second retained value");
        let retained_bytes = 2 * (size_of::<String>() + 3);

        // Assert
        assert_eq!(values.len(), 2);
        assert_eq!(values.as_slice(), ["one", "two"]);
        assert_eq!(values.accounted_bytes(), retained_bytes);
        assert_eq!(controls.current_query_memory_bytes(), retained_bytes);

        values.clear();
        assert!(values.is_empty());
        assert_eq!(values.accounted_bytes(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_leave_existing_accounting_unchanged_when_growth_is_rejected() {
        // Arrange
        let value_bytes = size_of::<u64>();
        let controls = controls_with_budget(value_bytes);
        let mut values = AccountedVec::try_new(&controls).expect("empty accounted vector");
        values
            .try_push_with(0, || 1_u64)
            .expect("first retained value");

        // Act
        let result = values.try_push_with(0, || 2_u64);

        // Assert
        assert!(result.is_err());
        assert_eq!(values.as_slice(), [1]);
        assert_eq!(values.accounted_bytes(), value_bytes);
        assert_eq!(controls.current_query_memory_bytes(), value_bytes);
        drop(values);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_new_accounting_when_retained_value_construction_fails() {
        // Arrange
        let controls = controls_with_budget(128);
        let mut values =
            AccountedVec::<String>::try_new(&controls).expect("empty accounted vector");

        // Act
        let result = values.try_push_with_result(16, || {
            Err(CassieError::Execution("decode failed".to_owned()))
        });

        // Assert
        assert!(result.is_err());
        assert!(values.is_empty());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
