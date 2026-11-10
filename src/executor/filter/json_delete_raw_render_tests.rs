use super::{
    render_array_without_index, render_array_without_keys, render_array_without_text,
    render_object_without_key, render_object_without_keys, render_observing, render_path, Progress,
};
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
fn should_match_existing_object_deletion_on_nested_raw_values() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let values = [
        (r#"{"a":1,"a":2,"drop":0}"#, "drop"),
        (r#"{"a":1,"\u0061":2,"drop":0}"#, "drop"),
        (
            r#"{"$serde_json::private::Number":"12","drop":"\ud800"}"#,
            "drop",
        ),
        (
            r#"{"drop":1,"a":"\u0000","b":[1.00,1e2,{"z":true}]}"#,
            "drop",
        ),
        (r#"{"drop":1e131072,"keep":1}"#, "drop"),
    ];
    // Act / Assert
    for (source, key) in values {
        let old = json_delete::delete_operator(
            &ScalarValue::Json(source.into()),
            &ScalarValue::Str(key.into()),
        )
        .expect("old deletion");
        let ScalarValue::Json(expected) = old else {
            panic!("JSON result")
        };
        let output = render_object_without_key(source, key, &controls).expect("raw deletion");
        // Assert
        assert_eq!(output.as_str(), expected, "{source}");
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_existing_array_index_deletion_with_leave_removed_values_opaque() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let values = [
        ("[0,1,2]", 1),
        ("[0,1,2]", -1),
        ("[0,1,2]", 99),
        (" \t[1]", i64::MIN),
        ("[]", i64::MAX),
        ("[7]", -1),
        (r#"["\ud800",{"x":1.00}]"#, 0),
    ];
    // Act / Assert
    for (source, index) in values {
        let old = json_delete::delete_operator(
            &ScalarValue::Json(source.into()),
            &ScalarValue::Int(index),
        )
        .expect("old deletion");
        let ScalarValue::Json(expected) = old else {
            panic!("JSON result")
        };
        let output = render_array_without_index(source, index, &controls).expect("raw deletion");
        // Assert
        assert_eq!(output.as_str(), expected, "{source} {index}");
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_direct_array_visitor_errors_before_index_selection() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let sources = ["[1,]", "[1,", "[1 2]", "[\"\\uX\"]"];
    // Act / Assert
    for source in sources {
        let old =
            json_delete::delete_operator(&ScalarValue::Json(source.into()), &ScalarValue::Int(0));
        let actual = render_array_without_index(source, 0, &controls);
        let error = match actual {
            Ok(_) => panic!("accepted {source}"),
            Err(error) => error.into_query_error(),
        };
        // Assert
        assert!(
            matches!((error, old),
            (crate::executor::QueryError::General(actual), Err(crate::executor::QueryError::General(expected))) if actual == expected),
            "{source}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_remove_every_matching_top_level_array_string() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"["x",{"x":1},"y","\u0078"]"#;
    let old = json_delete::delete_operator(
        &ScalarValue::Json(source.into()),
        &ScalarValue::Str("x".into()),
    )
    .expect("old deletion");
    let ScalarValue::Json(expected) = old else {
        panic!("JSON result")
    };
    // Act
    let actual = render_array_without_text(source, "x", &controls).expect("raw deletion");
    // Assert
    assert_eq!(actual.as_str(), expected);
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_treat_invalid_array_string_as_nonmatch_before_survivor_formatting() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"["\ud800","x"]"#;
    let old = json_delete::delete_operator(
        &ScalarValue::Json(source.into()),
        &ScalarValue::Str("x".into()),
    );
    // Act
    let actual = render_array_without_text(source, "x", &controls);
    // Assert
    let error = match actual {
        Ok(_) => panic!("invalid survivor accepted"),
        Err(error) => error.into_query_error(),
    };
    assert!(matches!((error, old),
        (crate::executor::QueryError::General(actual), Err(crate::executor::QueryError::General(expected))) if actual == expected));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_text_array_deletion_for_objects_with_arrays() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let keys = [Some("a".to_owned()), None, Some("b".to_owned())];
    let right = ScalarValue::Array(
        crate::types::SqlArray::from_elements(vec![
            crate::types::Value::String("a".into()),
            crate::types::Value::Null,
            crate::types::Value::String("b".into()),
        ])
        .expect("keys"),
    );
    // Act / Assert
    for source in [r#"{"a":1,"b":2,"c":3}"#, r#"["a",1,"b","c","a"]"#] {
        let old = json_delete::delete_operator(&ScalarValue::Json(source.into()), &right)
            .expect("old deletion");
        let ScalarValue::Json(expected) = old else {
            panic!("JSON result")
        };
        let result = if source.starts_with('\u{7b}') {
            render_object_without_keys(source, &keys, &controls)
        } else {
            render_array_without_keys(source, &keys, &controls)
        }
        .expect("raw deletion");
        // Assert
        assert_eq!(result.as_str(), expected);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_borrowed_native_key_arrays_for_both_container_shapes() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let keys = vec![
        crate::types::Value::String("a".into()),
        crate::types::Value::Null,
        crate::types::Value::String("b".into()),
    ];
    let right =
        ScalarValue::Array(crate::types::SqlArray::from_elements(keys.clone()).expect("keys"));
    // Act / Assert
    for source in [r#"{"a":1,"b":2,"c":3}"#, r#"["a",1,"b","c"]"#] {
        let old = json_delete::delete_operator(&ScalarValue::Json(source.into()), &right)
            .expect("old deletion");
        let ScalarValue::Json(expected) = old else {
            panic!("JSON result")
        };
        let result = if source.starts_with('\u{7b}') {
            super::render_object_without_native_keys(source, &keys, &controls)
        } else {
            super::render_array_without_native_keys(source, &keys, &controls)
        }
        .expect("raw deletion");
        // Assert
        assert_eq!(result.as_str(), expected);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_all_null_key_array_survivor_errors() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let keys = [None, None];
    let source = r#"["\ud800"]"#;
    let right = ScalarValue::Array(
        crate::types::SqlArray::from_elements(vec![
            crate::types::Value::Null,
            crate::types::Value::Null,
        ])
        .expect("keys"),
    );
    let old = json_delete::delete_operator(&ScalarValue::Json(source.into()), &right);
    // Act
    let result = render_array_without_keys(source, &keys, &controls);
    // Assert
    let actual = match result {
        Ok(_) => panic!("invalid survivor accepted"),
        Err(error) => error.into_query_error(),
    };
    assert!(matches!((actual, old),
        (crate::executor::QueryError::General(actual), Err(crate::executor::QueryError::General(expected))) if actual == expected));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_lazy_nested_path_deletion_with_missing_paths() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let cases = [
        (
            r#"{"a":{"b":1,"c":2}}"#,
            vec![Some("a".to_owned()), Some("b".to_owned())],
        ),
        (
            r#"{"a":[1,2,3]}"#,
            vec![Some("a".to_owned()), Some("-1".to_owned())],
        ),
        (r#"{"a":1}"#, vec![Some("missing".to_owned()), None]),
        ("{}", vec![None]),
        ("[]", vec![None]),
        (
            r#"{"a":[1,2]}"#,
            vec![Some("a".to_owned()), Some(" 0".to_owned())],
        ),
    ];
    // Act / Assert
    for (source, path) in cases {
        let values = path
            .iter()
            .map(|value| {
                value.as_ref().map_or(crate::types::Value::Null, |text| {
                    crate::types::Value::String(text.clone())
                })
            })
            .collect();
        let operand =
            ScalarValue::Array(crate::types::SqlArray::from_elements(values).expect("path"));
        let old = json_delete::delete_path(&ScalarValue::Json(source.into()), &operand)
            .expect("old deletion");
        let ScalarValue::Json(expected) = old else {
            panic!("JSON result")
        };
        let actual = render_path(source, &path, &controls).expect("raw path deletion");
        // Assert
        assert_eq!(actual.as_str(), expected);
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_reached_null_with_invalid_index_path_errors() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let cases = [
        (r#"{"a":1}"#, vec![Some("a".to_owned()), None]),
        (
            r#"{"a":[1]}"#,
            vec![Some("a".to_owned()), Some("2147483648".to_owned())],
        ),
        (
            r#"{"a":[1]}"#,
            vec![Some("a".to_owned()), Some("0 ".to_owned())],
        ),
    ];
    // Act / Assert
    for (source, path) in cases {
        let values = path
            .iter()
            .map(|value| {
                value.as_ref().map_or(crate::types::Value::Null, |text| {
                    crate::types::Value::String(text.clone())
                })
            })
            .collect();
        let operand =
            ScalarValue::Array(crate::types::SqlArray::from_elements(values).expect("path"));
        let old = match json_delete::delete_path(&ScalarValue::Json(source.into()), &operand) {
            Ok(_) => panic!("old path unexpectedly succeeded"),
            Err(error) => error.to_string(),
        };
        let actual = match render_path(source, &path, &controls) {
            Ok(_) => panic!("new path unexpectedly succeeded"),
            Err(error) => error.into_query_error().to_string(),
        };
        // Assert
        assert_eq!(actual, old);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_use_borrowed_native_path_values_without_copying_their_strings() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"{"a":{"b":1,"c":2}}"#;
    let values = vec![
        crate::types::Value::String("a".into()),
        crate::types::Value::String("b".into()),
    ];
    let path =
        ScalarValue::Array(crate::types::SqlArray::from_elements(values.clone()).expect("path"));
    let old = json_delete::delete_path(&ScalarValue::Json(source.into()), &path).expect("old path");
    let ScalarValue::Json(expected) = old else {
        panic!("JSON result")
    };
    // Act
    let actual = super::path::render_native_path(source, &values, &controls).expect("native path");
    // Assert
    assert_eq!(actual.as_str(), expected);
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_native_path_elements_before_scalar_document_rejection() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let values = vec![crate::types::Value::Int64(1)];
    let path =
        ScalarValue::Array(crate::types::SqlArray::from_elements(values.clone()).expect("path"));
    let old = json_delete::delete_path(&ScalarValue::Json("42".into()), &path);
    // Act
    let actual = super::path::render_native_path("42", &values, &controls);
    // Assert
    let actual = match actual {
        Ok(_) => panic!("invalid path accepted"),
        Err(error) => error.into_query_error().to_string(),
    };
    let old = match old {
        Ok(_) => panic!("old invalid path accepted"),
        Err(error) => error.to_string(),
    };
    assert_eq!(actual, old);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_format_the_rewritten_path_child_before_parent_siblings() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"{"a":"\ud800","z":{"x":0,"y":1e131072}}"#;
    let path = [Some("z".to_owned()), Some("x".to_owned())];
    let operand = ScalarValue::Array(
        crate::types::SqlArray::from_elements(vec![
            crate::types::Value::String("z".into()),
            crate::types::Value::String("x".into()),
        ])
        .expect("path"),
    );
    let old = json_delete::delete_path(&ScalarValue::Json(source.into()), &operand);
    // Act
    let actual = render_path(source, &path, &controls);
    // Assert
    assert!(
        matches!(old, Err(crate::executor::QueryError::General(message)) if message == "value overflows numeric format")
    );
    assert!(matches!(actual, Err(super::RenderError::Numeric)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_path_ancestors_after_actual_stack_move_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let mut source = r#"{"drop":1}"#.to_owned();
    for _ in 0..8 {
        source = format!(r#"{{"a":{source}}}"#);
    }
    let mut path = vec![Some("a".to_owned()); 8];
    path.push(Some("drop".to_owned()));
    let mut moved = 0;
    // Act
    let result = super::path::render_path_observing(&source, &path, &controls, || {
        moved += 1;
        cancellation.cancel();
    });
    // Assert
    assert_eq!(moved, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_measured_bottom_up_path_overlap() {
    // Arrange
    let source = r#"{"a":{"b":{"drop":1,"keep":2}},"z":3}"#;
    let path = [
        Some("a".to_owned()),
        Some("b".to_owned()),
        Some("drop".to_owned()),
    ];
    let baseline = controls(QueryCancellationHandle::new());
    let output = render_path(source, &path, &baseline).expect("path");
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
        let limited = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = render_path(source, &path, &limited);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_report_the_first_surviving_value_error_in_canonical_key_order() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"{"drop":0,"z":"\ud800","a":1e131072}"#;
    let old = json_delete::delete_operator(
        &ScalarValue::Json(source.into()),
        &ScalarValue::Str("drop".into()),
    );
    // Act
    let actual = render_object_without_key(source, "drop", &controls);
    // Assert
    assert!(
        matches!(old, Err(crate::executor::QueryError::General(message)) if message == "value overflows numeric format")
    );
    assert!(matches!(actual, Err(super::RenderError::Numeric)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_surviving_lone_surrogate_after_deletion() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let source = r#"{"drop":0,"keep":"\ud800"}"#;
    // Act
    let actual = render_object_without_key(source, "drop", &controls);
    // Assert
    let error = match actual {
        Ok(_) => panic!("invalid surviving string was accepted"),
        Err(error) => error.into_query_error(),
    };
    let old = json_delete::delete_operator(
        &ScalarValue::Json(source.into()),
        &ScalarValue::Str("drop".into()),
    );
    assert!(matches!((error, old),
        (crate::executor::QueryError::General(actual), Err(crate::executor::QueryError::General(expected))) if actual == expected));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_partial_output_after_actual_frame_move_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let nested = format!("{}0{}", "[".repeat(20), "]".repeat(20));
    let source = format!(r#"{{"drop":0,"keep":{nested}}}"#);
    let mut moves = 0;
    // Act
    let result = render_observing(&source, "drop", &controls, |event| {
        if matches!(event, Progress::FrameMove) {
            moves += 1;
            cancellation.cancel();
        }
    });
    // Assert
    assert_eq!(moves, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_the_measured_output_overlap_until_the_result_drops() {
    // Arrange
    let source = r#"{"drop":0,"a":[1,{"x":"hello"}],"b":2}"#;
    let baseline = controls(QueryCancellationHandle::new());
    let output = render_object_without_key(source, "drop", &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    // Assert
    assert!(baseline.current_query_memory_bytes() > 0);
    drop(output);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let limited = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = render_object_without_key(source, "drop", &limited);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_partial_output_after_actual_copy_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let source = format!(r#"{{"drop":0,"keep":"{}"}}"#, "a".repeat(4096));
    let mut copies = 0;
    // Act
    let result = render_observing(&source, "drop", &controls, |event| {
        if matches!(event, Progress::OutputCopy) {
            copies += 1;
            cancellation.cancel();
        }
    });
    // Assert
    assert_eq!(copies, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
