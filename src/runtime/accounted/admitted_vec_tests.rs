use super::AccountedVec;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::{cell::Cell, rc::Rc, time::Instant};

struct DropProbe {
    controls: QueryExecutionControls,
    observed: Rc<Cell<usize>>,
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.observed
            .set(self.controls.current_query_memory_bytes());
    }
}
fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_drop_prepared_value_before_its_lease_on_foreign_tracker_or_slot_failure() {
    // Arrange
    for foreign in [false, true] {
        let source = controls(if foreign { 1_024 } else { 16 });
        let other = controls(1_024);
        let reservation_controls = if foreign { &other } else { &source };
        let reservation = reservation_controls
            .reserve_query_memory(16)
            .expect("lease");
        let observed = Rc::new(Cell::new(0));
        let value = DropProbe {
            controls: reservation_controls.clone(),
            observed: Rc::clone(&observed),
        };
        let mut values = AccountedVec::try_new(&source).expect("vector");
        // Act
        let result = values.try_push_admitted(value, reservation, &source);
        // Assert
        assert!(result.is_err());
        assert!(observed.get() >= 16);
        drop(values);
        assert_eq!(source.current_query_memory_bytes(), 0);
        assert_eq!(other.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_transfer_prepared_lease_once_and_keep_it_until_value_drop() {
    // Arrange
    let controls = controls(1_024);
    let reservation = controls.reserve_query_memory(16).expect("lease");
    let observed = Rc::new(Cell::new(0));
    let value = DropProbe {
        controls: controls.clone(),
        observed: Rc::clone(&observed),
    };
    let mut values = AccountedVec::try_new(&controls).expect("vector");
    // Act
    values
        .try_push_admitted(value, reservation, &controls)
        .expect("insert");
    // Assert
    assert_eq!(
        controls.current_query_memory_bytes(),
        16 + std::mem::size_of::<DropProbe>()
    );
    drop(values);
    assert!(observed.get() >= 16);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
