use super::{parse, parse_observing, Progress};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::{json_delete, ScalarValue};
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
fn should_match_legacy_flat_brace_values_and_errors() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let inputs = [
        "{}",
        " {a,b} ",
        r#"{"a,b","NULL",NULL,"","a\\b"}"#,
        "{a,}",
        "{a,,b}",
        "{\"unclosed}",
        "{a b,c  }",
        "{ a\\ b, x}",
        r#"{N\ULL,"",NULL}"#,
        "{\u{2003}a\u{2003},b}",
        "{a} trailing",
        "{a\\",
    ];
    // Act / Assert
    for input in inputs {
        let old = json_delete::text_array(&ScalarValue::Str(input.into()))
            .map_err(|error| error.to_string());
        let actual = parse(input, &controls)
            .map(|path| path.expect("flat candidate").values().to_vec())
            .map_err(|error| error.to_string());
        assert_eq!(actual, old, "{input:?}");
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_a_partial_element_after_actual_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let mut copied = 0;
    // Act
    let result = parse_observing("{\"long element\"}", &controls, |event| {
        if matches!(event, Progress::Copy) {
            copied += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(copied, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_nested_dimension_and_json_array_forms_on_the_existing_path() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    // Act / Assert
    for input in ["{{a},{b}}", "[0:1]={a,b}", r#"["a",null]"#] {
        assert!(parse(input, &controls).expect("probe").is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_old_and_new_value_directories_after_actual_move_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let mut moved = 0;
    // Act
    let result = parse_observing("{a,b,c,d,e}", &controls, |event| {
        if matches!(event, Progress::DirectoryMove) {
            moved += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(moved, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_path_directory_and_string_overlap() {
    // Arrange
    let input = r#"{"alpha","beta",NULL,"delta","epsilon"}"#;
    let baseline = controls(QueryCancellationHandle::new());
    let path = parse(input, &baseline).expect("probe").expect("path");
    let peak = baseline.peak_query_memory_bytes();
    assert!(baseline.current_query_memory_bytes() > 0);
    drop(path);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = parse(input, &controls);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
