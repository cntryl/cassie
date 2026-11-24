//! Deferred source-directory lifetime probe for the chunk builder.
use super::*;
use crate::config::CassieRuntimeLimits;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

#[test]
fn should_keep_source_owner_during_last_actual_input_move() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = Arc::new(controls.reserve_query_memory(64).expect("source lease"));
    let weak = Arc::downgrade(&owner);
    let observed = Rc::new(Cell::new(false));
    OBSERVE_SOURCE.with(|slot| *slot.borrow_mut() = Some((weak.clone(), Rc::clone(&observed))));
    let retained = RetainedRows {
        rows: vec![BatchRow::new(Vec::new())],
        source_owner: Some(owner),
    };
    // Act
    let guard = build(retained, &controls).expect("chunk");
    // Assert
    assert!(observed.get());
    assert!(weak.upgrade().is_none());
    drop(guard);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
