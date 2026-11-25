use super::sort_iter;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_sort_arbitrary_iteration_order_by_rust_string_lexical_order() {
    // Arrange
    let input = ["é", "aa", "", "z", "\0", "a"];
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let sorted = sort_iter(input.len(), input.iter().copied(), &controls).expect("sort");
    // Assert
    assert_eq!(sorted.values, ["", "\0", "a", "aa", "z", "é"]);
    drop(sorted);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
