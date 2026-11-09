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

#[test]
fn should_transfer_flat_text_array_deletion_output_into_the_expression_owner() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"drop":1,"keep":2}"#.into());
    let right = ScalarValue::Json("{drop,NULL}".into());
    // Act
    let result = json_delete::controlled_operator(&left, &right, &controls, &owner)
        .expect("controlled deletion")
        .expect("flat path");
    // Assert
    assert!(matches!(&result, ScalarValue::Json(text) if text == r#"{"keep": 2}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_transfer_flat_text_path_deletion_output_into_the_expression_owner() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":{"b":1,"c":2}}"#.into());
    let right = ScalarValue::Str("{a,b}".into());
    // Act
    let result = json_delete::controlled_path(&left, &right, &controls, &owner)
        .expect("controlled path")
        .expect("flat path");
    // Assert
    assert!(matches!(&result, ScalarValue::Json(text) if text == r#"{"a": {"c": 2}}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_nested_string_backed_paths_on_the_legacy_route() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":1}"#.into());
    let right = ScalarValue::Str("{{a},{b}}".into());
    // Act
    let result = json_delete::controlled_path(&left, &right, &controls, &owner).expect("fallback");
    // Assert
    assert!(result.is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_flat_path_before_rejecting_a_scalar_document() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json("42".into());
    let right = ScalarValue::Str("{a,}".into());
    let expected = match json_delete::delete_path(&left, &right) {
        Ok(_) => panic!("invalid path accepted"),
        Err(error) => error.to_string(),
    };
    // Act
    let actual = match json_delete::controlled_path(&left, &right, &controls, &owner) {
        Ok(_) => panic!("invalid path accepted"),
        Err(error) => error.to_string(),
    };
    // Assert
    assert_eq!(actual, expected);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_empty_and_null_flat_path_semantics() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":1}"#.into());
    // Act / Assert
    for text in ["{}", "{NULL}"] {
        let right = ScalarValue::Str(text.into());
        let expected = json_delete::delete_path(&left, &right).map_err(|error| error.to_string());
        let actual = json_delete::controlled_path(&left, &right, &controls, &owner)
            .and_then(|value| {
                value.ok_or_else(|| {
                    crate::executor::QueryError::General("unexpected fallback".into())
                })
            })
            .map_err(|error| error.to_string());
        match (actual, expected) {
            (Ok(ScalarValue::Json(actual)), Ok(ScalarValue::Json(expected))) => {
                assert_eq!(actual, expected)
            }
            (Err(actual), Err(expected)) => assert_eq!(actual, expected),
            _ => panic!("path result mismatch for {text}"),
        }
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_the_parsed_path_when_cancelled_before_rendering() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":{"b":1}}"#.into());
    let right = ScalarValue::Str("{a,b}".into());
    json_delete::cancel_after_owned_path_once(cancellation.clone());
    // Act
    let result = json_delete::controlled_path(&left, &right, &controls, &owner);
    // Assert
    assert!(cancellation.is_cancelled());
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_measured_parsed_path_and_output_overlap_budget() {
    // Arrange
    let left = ScalarValue::Json(r#"{"a":{"b":1,"keep":2}}"#.into());
    let right = ScalarValue::Str("{a,b}".into());
    let baseline = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&baseline).expect("owner");
    let result = json_delete::controlled_path(&left, &right, &baseline, &owner).expect("deletion");
    let peak = baseline.peak_query_memory_bytes();
    drop(result);
    drop(owner);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
        let result = json_delete::controlled_path(&left, &right, &controls, &owner);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_transfer_json_array_key_operand_and_result_owners() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"drop":1,"keep":2}"#.into());
    let right = ScalarValue::Json(r#"["drop",null]"#.into());
    // Act
    let result = json_delete::controlled_operator(&left, &right, &controls, &owner)
        .expect("controlled deletion")
        .expect("JSON array operand");
    // Assert
    assert!(matches!(&result, ScalarValue::Json(text) if text == r#"{"keep": 2}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_malformed_json_array_operands_to_the_legacy_error_path() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":1}"#.into());
    let right = ScalarValue::Str(r#"["a",]"#.into());
    // Act
    let result = json_delete::controlled_path(&left, &right, &controls, &owner).expect("fallback");
    // Assert
    assert!(result.is_none());
    assert!(json_delete::delete_path(&left, &right).is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_the_json_array_arena_when_cancelled_before_rendering() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":{"b":1}}"#.into());
    let right = ScalarValue::Str(r#"["a","b"]"#.into());
    json_delete::cancel_after_owned_path_once(cancel.clone());
    // Act
    let result = json_delete::controlled_path(&left, &right, &controls, &owner);
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_json_array_arena_and_output_overlap_budget() {
    // Arrange
    let left = ScalarValue::Json(r#"{"a":{"b":1,"keep":2}}"#.into());
    let right = ScalarValue::Str(r#"["a","b"]"#.into());
    let baseline = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&baseline).expect("owner");
    let result = json_delete::controlled_path(&left, &right, &baseline, &owner).expect("deletion");
    let peak = baseline.peak_query_memory_bytes();
    drop(result);
    drop(owner);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
        let result = json_delete::controlled_path(&left, &right, &controls, &owner);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_json_array_path_null_error_and_empty_root_order() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let owner = crate::executor::filter::EvaluationMemory::new(&controls).expect("owner");
    let right = ScalarValue::Str(r#"["a",null]"#.into());
    // Act / Assert
    for document in [r#"{"a":{"b":1}}"#, "{}"] {
        let left = ScalarValue::Json(document.into());
        let expected = json_delete::delete_path(&left, &right).map_err(|error| error.to_string());
        let actual = json_delete::controlled_path(&left, &right, &controls, &owner)
            .and_then(|value| {
                value.ok_or_else(|| {
                    crate::executor::QueryError::General("unexpected fallback".into())
                })
            })
            .map_err(|error| error.to_string());
        match (actual, expected) {
            (Ok(ScalarValue::Json(actual)), Ok(ScalarValue::Json(expected))) => {
                assert_eq!(actual, expected)
            }
            (Err(actual), Err(expected)) => assert_eq!(actual, expected),
            _ => panic!("path result mismatch for {document}"),
        }
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
