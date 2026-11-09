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
fn should_retain_value_mode_json_string_and_null_elements() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let input = "\u{2003}[\"a\", null, \"b\\u0063\"]\u{2003}";
    // Act
    let path = parse(input, &controls)
        .expect("candidate")
        .expect("string array");
    // Assert
    assert_eq!(path.len(), 3);
    assert_eq!(path.get(0).expect("element"), Some("a"));
    assert_eq!(path.get(1).expect("element"), None);
    assert_eq!(path.get(2).expect("element"), Some("bc"));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(path);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_malformed_nonstring_sentinel_and_dimension_forms_to_legacy() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let inputs = [
        r#"["a",]"#,
        "[1]",
        "[[\"a\"]]",
        r#"[{"$serde_json::private::Number":"bad"}]"#,
        r#"[{"$serde_json::private::Number":"1"}]"#,
        "[0:1]={a,b}",
    ];
    // Act / Assert
    for input in inputs {
        assert!(
            parse(input, &controls).expect("candidate").is_none(),
            "{input}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_nested_raw_value_sentinel_strings() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let inner = serde_json::json!({"$serde_json::private::RawValue":"\"ok\""}).to_string();
    let input = serde_json::json!([{"$serde_json::private::RawValue":inner}]).to_string();
    let old = crate::types::array::parse_text_array(&input, &crate::types::DataType::Text)
        .expect("legacy");
    // Act
    let actual = parse(&input, &controls)
        .expect("candidate")
        .expect("text array");
    // Assert
    assert_eq!(actual.len(), 1);
    assert_eq!(
        actual.get(0).expect("element"),
        old.as_array().expect("array")[0].as_str()
    );
    assert_eq!(actual.get(0).expect("element"), Some("ok"));
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_nested_sentinel_error_before_later_array_syntax_to_legacy() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let input = r#"[{"$serde_json::private::RawValue":"["},]"#;
    let old = crate::types::array::parse_text_array(input, &crate::types::DataType::Text);
    // Act
    let actual = parse(input, &controls).expect("candidate");
    // Assert
    assert!(old.is_err());
    assert!(actual.is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_the_arena_after_actual_parse_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let mut entered = 0;
    // Act
    let result = parse_observing(r#"["a","b"]"#, &controls, || {
        entered += 1;
        cancellation.cancel();
    });
    // Assert
    assert_eq!(entered, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_existing_value_mode_text_arrays() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let inputs = [
        r#"[]"#,
        r#"["",null,"NULL","a\\b"]"#,
        r#"["é", "\ud834\udd1e"]"#,
    ];
    // Act / Assert
    for input in inputs {
        let old = crate::types::array::parse_text_array(input, &crate::types::DataType::Text)
            .expect("legacy array");
        let expected = old.as_array().expect("array");
        let actual = parse(input, &controls)
            .expect("candidate")
            .expect("text array");
        assert_eq!(actual.len(), expected.len());
        for (index, value) in expected.iter().enumerate() {
            assert_eq!(actual.get(index).expect("element"), value.as_str());
        }
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_the_measured_value_arena_overlap_budget() {
    // Arrange
    let input = r#"["alpha",null,"beta","gamma"]"#;
    let baseline = controls(QueryCancellationHandle::new());
    let path = parse(input, &baseline).expect("candidate").expect("array");
    let peak = baseline.peak_query_memory_bytes();
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
