use super::{parse, parse_observing, Progress};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use serde_json::value::RawValue;
use std::time::Instant;

fn controls(cancel: QueryCancellationHandle) -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel,
    )
}

#[test]
fn should_index_reached_keys_and_leave_raw_child_strings_undecoded() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let input = r#"{"$serde_json::private::Number":"\ud800","a":1,"a":2}"#;
    // Act
    let fields = parse(input, &controls).expect("raw object");
    // Assert
    assert_eq!(
        fields
            .last_value("$serde_json::private::Number", &controls)
            .expect("lookup"),
        Some(r#""\ud800""#)
    );
    assert_eq!(
        fields.last_value("a", &controls).expect("lookup"),
        Some("2")
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(fields);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_report_the_first_reached_key_error_before_later_raw_syntax() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let input = "{\n \"é\":1, \"\\ud800\": 2, \"later\":[}";
    let expected = serde_json::from_str::<std::collections::BTreeMap<String, Box<RawValue>>>(input)
        .expect_err("invalid key")
        .to_string();
    // Act
    let Err(actual) = parse(input, &controls) else {
        panic!("invalid key was accepted")
    };
    // Assert
    assert_eq!(actual.to_string(), expected);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_key_and_directory_owners_after_actual_entry_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let mut entries = 0;
    // Act
    let result = parse_observing(r#"{"a":1,"b":2}"#, &controls, |progress| {
        if matches!(progress, Progress::Entry) {
            entries += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(entries, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_raw_object_error_positions_and_categories() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let values = [
        "{",
        "{,}",
        "{\"a\"}",
        "{\"a\":}",
        "{\"a\":1,}",
        "{\"a\":1,",
        "{\"a\":1 \"b\":2}",
        "{\"a\": [1,]}",
        "{\n \"é\":1, \"\\ud800\":0}",
    ];
    // Act / Assert
    for value in values {
        let expected =
            serde_json::from_str::<std::collections::BTreeMap<String, Box<RawValue>>>(value)
                .expect_err("invalid object")
                .to_string();
        let Err(actual) = parse(value, &controls) else {
            panic!("accepted {value:?}")
        };
        assert_eq!(actual.to_string(), expected, "{value:?}");
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_both_object_directories_after_actual_move_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let mut moved = 0;
    // Act
    let result = parse_observing(
        r#"{"a":1,"b":2,"c":3,"d":4,"e":5}"#,
        &controls,
        |progress| {
            if matches!(progress, Progress::DirectoryMove) {
                moved += 1;
                cancel.cancel();
            }
        },
    );
    // Assert
    assert_eq!(moved, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_object_key_and_directory_overlap_budget() {
    // Arrange
    let input = r#"{"é":1,"a":2,"b":3,"c":4,"d":5}"#;
    let baseline = controls(QueryCancellationHandle::new());
    let fields = parse(input, &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(fields);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let limited = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = parse(input, &limited);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_order_surviving_last_duplicate_keys_by_jsonb_length_then_bytes() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"{"aa":1,"b":2,"a":3,"é":4,"\u0061":5}"#;
    let fields = parse(source, &controls).expect("object");
    // Act
    let order = fields.ordered(&controls).expect("canonical order");
    let entries = order
        .indices()
        .iter()
        .map(|index| {
            let field = &fields.entries[*index];
            (field.key.get().as_str(), &source[field.value.clone()])
        })
        .collect::<Vec<_>>();
    // Assert
    assert_eq!(entries, [("a", "5"), ("b", "2"), ("aa", "1"), ("é", "4")]);
    drop(order);
    drop(fields);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_order_directories_after_actual_key_comparison_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let fields = parse(r#"{"ac":1,"ab":2,"aa":3}"#, &controls).expect("object");
    let retained = controls.current_query_memory_bytes();
    let mut compared = 0;
    // Act
    let result = super::order::sort_observing(&fields, &controls, || {
        compared += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(compared, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), retained);
    drop(fields);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_the_measured_two_buffer_order_overlap() {
    // Arrange
    let input = r#"{"ac":1,"ab":2,"aa":3,"zz":4,"bb":5}"#;
    let baseline = controls(QueryCancellationHandle::new());
    let fields = parse(input, &baseline).expect("object");
    let retained = baseline.current_query_memory_bytes();
    let order = fields.ordered(&baseline).expect("order");
    let peak = baseline.peak_query_memory_bytes();
    assert!(peak >= retained + 2 * fields.entries.len() * std::mem::size_of::<usize>());
    drop(order);
    drop(fields);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let limited = QueryExecutionControls::from_limits(&limits, Instant::now());
        let actual = match parse(input, &limited) {
            Ok(fields) => {
                let result = fields.ordered(&limited);
                let ok = result.is_ok();
                drop(result);
                ok
            }
            Err(_) => false,
        };
        assert_eq!(actual, succeeds);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_fields_and_order_owned_together_during_output() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"{"b":1,"a":2,"a":3}"#;
    let fields = parse(source, &controls).expect("object");
    // Act
    let ordered = fields.into_ordered(&controls).expect("ordered object");
    // Assert
    assert_eq!(ordered.get(0), Some(("a", "3")));
    assert_eq!(ordered.get(1), Some(("b", "1")));
    assert_eq!(ordered.get(2), None);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(ordered);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
