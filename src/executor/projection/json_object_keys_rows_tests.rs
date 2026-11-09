use super::super::row::PreparedRow;
use super::Rows;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

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
fn row(controls: &QueryExecutionControls) -> PreparedRow {
    PreparedRow {
        row: BatchRow::from_projected_values(Vec::new()),
        memory: controls.reserve_query_memory(0).expect("row lease"),
    }
}

#[test]
fn should_release_prepared_row_on_foreign_tracker_without_insertion() {
    // Arrange
    let controls = controls(1_000_000);
    let foreign = self::controls(1_000_000);
    let mut rows = Rows::new(&controls).expect("rows");
    let mut prepared = row(&foreign);
    prepared.memory.try_grow(16).expect("foreign payload");
    assert_eq!(foreign.current_query_memory_bytes(), 16);
    // Act
    let result = rows.push(prepared, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(foreign.current_query_memory_bytes(), 0);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_existing_rows_owned_when_replacement_cannot_be_admitted() {
    // Arrange
    let slot = std::mem::size_of::<BatchRow>();
    let controls = controls(slot * 4);
    let mut rows = Rows::new(&controls).expect("rows");
    for _ in 0..4 {
        rows.push(row(&controls), &controls).expect("initial row");
    }
    assert_eq!(controls.current_query_memory_bytes(), slot * 4);
    // Act
    let result = rows.push(row(&controls), &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), slot * 4);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
