use super::{Cell, CellError, Identity};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_parse_cells_with_identity_specific_text() {
    // Arrange
    let controls = controls();
    let source = r#" {"$serde_json::private::Number" : "7"} "#;
    // Act
    let json = Cell::parse(source, Identity::Json, &controls).expect("JSON cell");
    let jsonb = Cell::parse(source, Identity::Jsonb, &controls).expect("JSONB cell");
    // Assert
    assert_eq!(json.identity(), Identity::Json);
    assert_eq!(json.text(), source);
    assert_eq!(jsonb.identity(), Identity::Jsonb);
    assert_eq!(jsonb.text(), r#"{"$serde_json::private::Number": "7"}"#);
    let null = Cell::parse("null", Identity::Jsonb, &controls).expect("JSON null");
    assert_eq!(null.text(), "null");
    drop(null);
    drop(json);
    drop(jsonb);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_output_tracker_before_adopting_the_cell() {
    // Arrange
    let first = controls();
    let second = controls();
    let mut sink = crate::runtime::accounted::compact_json::Sink::for_arena(&first).expect("sink");
    sink.write("{}", &mut |_| {}).expect("document");
    let output = sink.finish_output();
    // Act
    let result = Cell::from_output(output, Identity::Json, &second);
    // Assert
    assert!(result.is_err());
    drop(result);
    assert_eq!(first.current_query_memory_bytes(), 0);
    assert_eq!(second.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_tracker_when_cloning_a_cell() {
    // Arrange
    let first = controls();
    let second = controls();
    let cell = Cell::parse("{}", Identity::Json, &first).expect("cell");
    // Act
    let result = cell.try_clone(&second);
    // Assert
    assert!(matches!(
        result,
        Err(CellError::Control(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(second.current_query_memory_bytes(), 0);
    drop(cell);
    assert_eq!(first.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_independently_admitted_clone_after_source_cell_drops() {
    // Arrange
    let controls = controls();
    let cell = Cell::parse(r#"{"a":"payload"}"#, Identity::Jsonb, &controls).expect("cell");
    let clone = cell.try_clone(&controls).expect("clone");
    let retained = controls.current_query_memory_bytes();
    // Act
    drop(cell);
    // Assert
    assert!(retained > 0);
    assert!(controls.current_query_memory_bytes() < retained);
    assert!(controls.current_query_memory_bytes() > 0);
    assert_eq!(clone.text(), r#"{"a": "payload"}"#);
    drop(clone);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_partial_clone_after_actual_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let cell = Cell::parse(r#"{"a":"payload"}"#, Identity::Json, &controls).expect("cell");
    let original = controls.current_query_memory_bytes();
    let mut copies = 0;
    // Act
    let result = cell.try_clone_observing(&controls, || {
        copies += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copies, 1);
    assert!(matches!(
        result,
        Err(CellError::Control(crate::app::CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), original);
    drop(cell);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_source_clone_overlap_budget() {
    // Arrange
    let source = format!("\"{}\"", "x".repeat(4096));
    let baseline = controls();
    let cell = Cell::parse(&source, Identity::Json, &baseline).expect("cell");
    let clones = (0..16)
        .map(|_| cell.try_clone(&baseline).expect("clone"))
        .collect::<Vec<_>>();
    let peak = baseline.peak_query_memory_bytes();
    drop(clones);
    drop(cell);

    // Act
    let results: Vec<_> = [(peak, true), (peak - 1, false)]
        .into_iter()
        .map(|(budget, succeeds)| {
            let limits = CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                query_timeout_ms: 0,
                ..CassieRuntimeLimits::default()
            };
            let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
            let cell = Cell::parse(&source, Identity::Json, &controls).expect("source");
            let clones = (0..15)
                .map(|_| cell.try_clone(&controls).expect("clone"))
                .collect::<Vec<_>>();
            let result = cell.try_clone(&controls);
            let accepted = result.is_ok();
            drop(result);
            drop(clones);
            drop(cell);
            (succeeds, accepted, controls.current_query_memory_bytes())
        })
        .collect();

    // Assert
    for (succeeds, accepted, memory) in results {
        assert_eq!(accepted, succeeds);
        assert_eq!(memory, 0);
    }
}

#[test]
fn should_leave_no_memory_after_jsonb_parse_errors() {
    // Arrange
    let controls = controls();

    // Act
    let results: Vec<_> = [(r#"{"a":"\u0000"}"#, true), (r#"{"a":1e131072}"#, false)]
        .into_iter()
        .map(|(source, nul)| {
            let result = Cell::parse(source, Identity::Jsonb, &controls);
            let matched = matches!(
                (result, nul),
                (Err(CellError::Nul), true) | (Err(CellError::Numeric), false)
            );
            (matched, controls.current_query_memory_bytes())
        })
        .collect();

    // Assert
    for (matched, memory) in results {
        assert!(matched);
        assert_eq!(memory, 0);
    }
}

#[test]
fn should_release_all_owners_after_actual_raw_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut copies = 0;
    // Act
    let result = Cell::parse_observing(r#"{"a":"payload"}"#, Identity::Json, &controls, || {
        copies += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copies, 1);
    assert!(result.is_err());
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_cell_creation_overlap_budget() {
    // Arrange
    let source = r#"{"$serde_json::private::Number":{"nested":[1,2,3]}}"#;
    let baseline = controls();
    let cell = Cell::parse(source, Identity::Jsonb, &baseline).expect("cell");
    let peak = baseline.peak_query_memory_bytes();
    drop(cell);
    assert_eq!(baseline.current_query_memory_bytes(), 0);

    // Act
    let results: Vec<_> = [(peak, true), (peak - 1, false)]
        .into_iter()
        .map(|(budget, succeeds)| {
            let limits = CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                query_timeout_ms: 0,
                ..CassieRuntimeLimits::default()
            };
            let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
            let result = Cell::parse(source, Identity::Jsonb, &controls);
            let accepted = result.is_ok();
            drop(result);
            (succeeds, accepted, controls.current_query_memory_bytes())
        })
        .collect();

    // Assert
    for (succeeds, accepted, memory) in results {
        assert_eq!(accepted, succeeds);
        assert_eq!(memory, 0);
    }
}
