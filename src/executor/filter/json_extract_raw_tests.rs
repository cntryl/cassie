use super::{
    render_selected, render_selected_observing, select, select_observing, Output, Progress,
    Selector,
};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        QueryCancellationHandle::new(),
    )
}

#[test]
fn should_select_the_last_decoded_object_key_without_reformatting() {
    // Arrange
    let controls = controls();
    let source = r#" {"a": 0,"\u0061": { "n" : 1e+3 },"other":"\ud800"} "#;
    // Act
    let result = select(source, Selector::Key("a"), false, &controls).expect("selection");
    // Assert
    assert_eq!(result, Some(r#"{ "n" : 1e+3 }"#));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_index_with_scalar_selection_rules() {
    // Arrange
    let controls = controls();
    let cases = [
        ("[1, 2, 3]", Selector::Index(-1), false, Some("3")),
        ("[1, 2, 3]", Selector::Index(i64::MIN), false, None),
        ("[1, 2, 3]", Selector::Path("  +1"), false, Some("2")),
        ("[1, 2, 3]", Selector::Path("1 "), false, None),
        ("[1, 2, 3]", Selector::Path("2147483648"), false, None),
        (" 42 ", Selector::Index(-1), true, Some("42")),
        (" 42 ", Selector::Index(0), false, None),
        ("{\"0\":7}", Selector::Path("0"), true, Some("7")),
    ];
    // Act / Assert
    for (source, selector, jsonb, expected) in cases {
        // Assert
        assert_eq!(
            select(source, selector, jsonb, &controls).expect("selection"),
            expected
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_validate_the_complete_raw_document_before_selecting() {
    // Arrange
    let controls = controls();
    let source = r#"{"a":1,"later":[}"#;
    // Act
    let result = select(source, Selector::Key("a"), false, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_selection_memory_after_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    // Act
    let result = select(r#"{"a":1}"#, Selector::Key("a"), false, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_selection_memory_after_actual_scan_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut bytes = 0;
    // Act
    let result = select_observing(
        r#"{"a":[1,2,3]}"#,
        Selector::Key("a"),
        false,
        &controls,
        |progress| {
            if matches!(progress, Progress::Input) {
                bytes += 1;
                if bytes == 5 {
                    cancellation.cancel();
                }
            }
        },
    );
    // Assert
    assert_eq!(bytes, 5);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_raw_decoded_with_jsonb_formatted_output() {
    // Arrange
    let controls = controls();
    let cases = [
        (r#"{ "x" : 1e+3 }"#, false, false, Some(r#"{ "x" : 1e+3 }"#)),
        (r#"{ "x" : 1e+3 }"#, false, true, Some(r#"{"x": 1000}"#)),
        (r#""a\nb""#, true, false, Some("a\nb")),
        ("null", true, true, None),
        ("null", false, true, Some("null")),
    ];
    // Act / Assert
    for (raw, as_text, jsonb, expected) in cases {
        let output = render_selected(raw, as_text, jsonb, &controls).expect("output");
        match (output, expected) {
            (Output::Null, None) => {}
            // Assert
            (Output::Text(value), Some(expected)) => assert_eq!(value.as_str(), expected),
            _ => panic!("output mismatch for {raw}"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_legacy_path_index_boundaries_with_duplicate_selection() {
    // Arrange
    let controls = controls();
    let cases = [
        (r#"{"a":0,"\u0061":2}"#, "a"),
        ("[0,1]", "2147483647"),
        ("[0,1]", "-2147483648"),
        ("[0,1]", "2147483648"),
        ("[0,1]", "-2147483649"),
        ("[0,1]", "1 "),
        ("[0,1]", "\t+1"),
    ];
    // Act / Assert
    for (source, key) in cases {
        let legacy = crate::executor::filter::json_extract::evaluate_path(
            source,
            [Some(key)],
            false,
            false,
            None,
        )
        .expect("legacy path");
        let actual = select(source, Selector::Path(key), false, &controls).expect("selection");
        match (legacy, actual) {
            (crate::executor::filter::ScalarValue::Null, None) => {}
            (crate::executor::filter::ScalarValue::Json(expected), Some(actual)) => {
                // Assert
                assert_eq!(actual, expected);
            }
            _ => panic!("selection mismatch for {source:?} / {key:?}"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_legacy_reached_key_error_with_keep_nested_spans_borrowed() {
    // Arrange
    let controls = controls();
    let invalid = r#"{"ok":1,"\ud800":2}"#;
    let expected = crate::executor::filter::json_extract::evaluate_path(
        invalid,
        [Some("ok")],
        false,
        false,
        None,
    )
    .expect_err("invalid key")
    .to_string();
    // Act
    let actual = select(invalid, Selector::Path("ok"), false, &controls)
        .expect_err("invalid key")
        .into_query_error()
        .to_string();
    let source = String::from(r#"{"a":{"b":[0,{"c":17}]}}"#);
    let first = select(&source, Selector::Path("a"), false, &controls)
        .expect("a")
        .expect("a");
    let second = select(first, Selector::Path("b"), false, &controls)
        .expect("b")
        .expect("b");
    let third = select(second, Selector::Path("-1"), false, &controls)
        .expect("index")
        .expect("index");
    let final_value = select(third, Selector::Path("c"), false, &controls).expect("c");
    // Assert
    assert_eq!(actual, expected);
    assert_eq!(final_value, Some("17"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_legacy_text_extraction_error_for_lone_surrogate() {
    // Arrange
    let controls = controls();
    let raw = r#""\ud800""#;
    let source = format!("[{raw}]");
    let legacy = crate::executor::filter::json_extract::evaluate(
        &crate::executor::filter::ScalarValue::Json(source),
        &crate::executor::filter::ScalarValue::Int(0),
        false,
        true,
        false,
    )
    .expect_err("invalid string")
    .to_string();
    // Act
    let actual = match render_selected(raw, true, false, &controls) {
        Ok(_) => panic!("accepted lone surrogate"),
        Err(error) => error.into_query_error().to_string(),
    };
    // Assert
    assert_eq!(actual, legacy);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_decoded_with_output_owners_after_actual_copy_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut copied = 0;
    // Act
    let result = render_selected_observing(
        r#""a long decoded string""#,
        true,
        false,
        &controls,
        |progress| {
            if matches!(
                progress,
                crate::runtime::accounted::compact_json::Progress::Copy
            ) {
                copied += 1;
                cancellation.cancel();
            }
        },
    );
    // Assert
    assert_eq!(copied, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_decoded_source_with_output_overlap() {
    // Arrange
    let raw = r#""abcdefghijklmno\nαβγ""#;
    let baseline = controls();
    let output = render_selected(raw, true, false, &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(output);
    // Assert
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = render_selected(raw, true, false, &controls);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
