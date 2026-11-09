use super::{parse, parse_observing};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(cancel: QueryCancellationHandle) -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel,
    )
}

#[test]
fn should_match_existing_path_index_syntax_and_bounds() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let inputs = [
        "",
        " ",
        "0",
        "-0",
        "+1",
        " 1",
        "1 ",
        "++1",
        "-2147483648",
        "2147483647",
        "2147483648",
        "-2147483649",
        "0000000000000000000001",
    ];
    // Act / Assert
    for input in inputs {
        let old = super::super::super::super::super::parse_path_index(input, 3, 2)
            .map_err(|error| error.to_string());
        let new = parse(input, 3, 2, &controls).map_err(|error| error.to_string());
        assert_eq!(new, old, "{input:?}");
    }
}

#[test]
fn should_cancel_after_scanning_an_actual_path_index_byte() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let mut visited = 0;
    // Act
    let result = parse_observing("000000001", 3, 1, &controls, || {
        visited += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(visited, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
