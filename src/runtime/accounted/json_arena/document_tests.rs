use std::time::Instant;

use super::{Document, DocumentError, DocumentProgress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_build_ordinary_nested_values_and_replace_duplicate_keys() {
    // Arrange
    let controls = controls();
    let input = r#"{"a":[null,true,"x",1E2],"a":{"b":false}}"#;

    // Act
    let parsed = Document::parse(input, &controls).expect("document");

    // Assert
    let child = parsed
        .forest
        .member(parsed.root, "a")
        .expect("lookup")
        .expect("a");
    let boolean = parsed
        .forest
        .member(child, "b")
        .expect("lookup")
        .expect("b");
    assert_eq!(
        parsed.forest.boolean_value(boolean).expect("value"),
        Some(false)
    );
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_accept_empty_containers_and_later_sentinel_shaped_keys() {
    // Arrange
    let controls = controls();
    let inputs = [
        "[]",
        "{}",
        r#"{"normal":1,"$serde_json::private::Number":"ordinary"}"#,
    ];

    // Act / Assert
    for input in inputs {
        let parsed = Document::parse(input, &controls).expect("document");
        drop(parsed);
        assert_eq!(controls.current_query_memory_bytes(), 0, "{input}");
    }
}

#[test]
fn should_retain_each_ordinary_scalar_and_nested_container_in_order() {
    // Arrange
    let controls = controls();
    let input = r#"[null,true,false,"hé",1E2,{},[]]"#;

    // Act
    let parsed = Document::parse(input, &controls).expect("document");

    // Assert
    {
        let get = |index| {
            parsed
                .forest
                .element(parsed.root, index)
                .expect("element")
                .expect("present")
        };
        assert_eq!(
            parsed.forest.boolean_value(get(1)).expect("true"),
            Some(true)
        );
        assert_eq!(
            parsed.forest.boolean_value(get(2)).expect("false"),
            Some(false)
        );
        assert_eq!(parsed.forest.text_value(get(3)).expect("text"), Some("hé"));
        assert_eq!(
            parsed.forest.number_value(get(4)).expect("number"),
            Some("1e+2")
        );
        assert_eq!(
            parsed.forest.member(get(5), "missing").expect("object"),
            None
        );
        assert_eq!(parsed.forest.element(get(6), 0).expect("array"), None);
        assert_eq!(parsed.forest.element(parsed.root, 7).expect("end"), None);
    }
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_ordinary_container_error_categories_and_positions() {
    // Arrange
    let controls = controls();
    let inputs = [
        "[",
        "[1",
        "[1,",
        "[1,]",
        "[, ]",
        "[1 2]",
        "{",
        "{\"a\"",
        "{\"a\"}",
        "{\"a\":",
        "{\"a\":1",
        "{\"a\":1,",
        "{\"a\":1,}",
        "{,}",
        "{\"a\":1,2}",
        "{\"a\":1 \"b\":2}",
        "1 x",
    ];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
        let Err(error) = Document::parse(input, &controls) else {
            panic!("accepted {input:?}");
        };
        let (category, line, column) = error.test_diagnostic().expect("diagnostic");
        assert_eq!(
            (category, line, column),
            (expected.classify(), expected.line(), expected.column()),
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0, "{input:?}");
    }
}

#[test]
fn should_match_serde_nesting_limit_without_a_new_depth_limit() {
    // Arrange
    let controls = controls();
    let accepted = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    let rejected = format!("{}0{}", "[".repeat(128), "]".repeat(128));

    // Act / Assert
    let parsed = Document::parse(&accepted, &controls).expect("127 nested arrays");
    drop(parsed);
    let expected = serde_json::from_str::<serde_json::Value>(&rejected).expect_err("serde limit");
    let Err(error) = Document::parse(&rejected, &controls) else {
        panic!("accepted depth");
    };
    let (category, line, column) = error.test_diagnostic().expect("diagnostic");
    assert_eq!(
        (category, line, column),
        (expected.classify(), expected.line(), expected.column())
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_parse_nested_raw_value_sentinels_without_losing_number_spelling() {
    // Arrange
    let controls = controls();
    let input =
        r#"{"$serde_json::private::RawValue":"{\"$serde_json::private::RawValue\":\"1E2\"}"}"#;
    let expected = serde_json::from_str::<serde_json::Value>(input)
        .expect("serde")
        .to_string();

    // Act
    let parsed = Document::parse(input, &controls).expect("document");

    // Assert
    assert_eq!(
        parsed.forest.number_value(parsed.root).expect("number"),
        Some(expected.as_str())
    );
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_raw_sentinel_nested_errors_and_outer_map_precedence() {
    // Arrange
    let controls = controls();
    let inputs = [
        r#"{"$serde_json::private::RawValue":"[1,]","other":1}"#,
        r#"{"$serde_json::private::RawValue":"","other":1}"#,
        r#"{"$serde_json::private::RawValue":"1","other":1}"#,
        r#"{"$serde_json::private::RawValue":"1" "other":1}"#,
        r#"{"$serde_json::private::RawValue":"\q"}"#,
        r#"{"$serde_json::private::RawValue":null}"#,
    ];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
        let Err(error) = Document::parse(input, &controls) else {
            panic!("accepted {input:?}");
        };
        assert_eq!(
            error.test_diagnostic(),
            Some((expected.classify(), expected.line(), expected.column())),
            "{input:?}"
        );
        if expected.classify() == serde_json::error::Category::Data {
            assert_eq!(
                error.test_message(),
                Some(expected.to_string()),
                "{input:?}"
            );
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reset_the_serde_depth_limit_for_nested_raw_sources() {
    // Arrange
    let controls = controls();
    let nested = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    let encoded = serde_json::to_string(&nested).expect("encode source");
    let input = format!(r#"{{"$serde_json::private::RawValue":{encoded}}}"#);

    // Act / Assert
    assert!(serde_json::from_str::<serde_json::Value>(&input).is_ok());
    let parsed = Document::parse(&input, &controls).expect("nested source depth");
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_wrap_nested_raw_depth_errors_with_inner_position_and_message() {
    // Arrange
    let controls = controls();
    let nested = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    let encoded = serde_json::to_string(&nested).expect("encode source");
    let input = format!(r#"{{"$serde_json::private::RawValue":{encoded}}}"#);
    let expected = serde_json::from_str::<serde_json::Value>(&input).expect_err("serde");

    // Act
    let Err(error) = Document::parse(&input, &controls) else {
        panic!("accepted depth");
    };

    // Assert
    assert_eq!(
        error.test_diagnostic(),
        Some((expected.classify(), expected.line(), expected.column()))
    );
    assert_eq!(error.test_message(), Some(expected.to_string()));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_wrapped_number_sentinel_error_through_raw_nesting() {
    // Arrange
    let controls = controls();
    let nested = r#"{"$serde_json::private::Number":"-xY","other":1}"#;
    let encoded = serde_json::to_string(nested).expect("encode source");
    let input = format!(r#"{{"$serde_json::private::RawValue":{encoded},"outer":1}}"#);
    let expected = serde_json::from_str::<serde_json::Value>(&input).expect_err("serde");

    // Act
    let Err(error) = Document::parse(&input, &controls) else {
        panic!("accepted nested error");
    };

    // Assert
    assert_eq!(
        error.test_diagnostic(),
        Some((expected.classify(), expected.line(), expected.column()))
    );
    assert_eq!(error.test_message(), Some(expected.to_string()));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_each_nested_raw_error_cause_and_message() {
    // Arrange
    let controls = controls();
    let nested_sources = [
        "[1,]",
        "trXe",
        "1e+x",
        r#""\q""#,
        r#"{"a":1,}"#,
        r#"{"$serde_json::private::Number":"-xY"}"#,
        r#"{"$serde_json::private::RawValue":null}"#,
        r#"{"$serde_json::private::RawValue":"[1,]"}"#,
    ];

    // Act / Assert
    for nested in nested_sources {
        let encoded = serde_json::to_string(nested).expect("encode source");
        let input = format!(r#"{{"$serde_json::private::RawValue":{encoded},"outer":1}}"#);
        let expected = serde_json::from_str::<serde_json::Value>(&input).expect_err("serde");
        let Err(error) = Document::parse(&input, &controls) else {
            panic!("accepted {nested:?}");
        };
        assert_eq!(
            error.test_diagnostic(),
            Some((expected.classify(), expected.line(), expected.column())),
            "{nested:?}"
        );
        assert_eq!(
            error.test_message(),
            Some(expected.to_string()),
            "{nested:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_non_string_raw_sentinel_errors_against_serde() {
    // Arrange
    let controls = controls();
    let payloads = [
        "null", "true", "0", "-0", "1.0", "[]", "{}", "truX", "1e+x", "x",
    ];

    // Act / Assert
    for payload in payloads {
        let input = format!(r#"{{"$serde_json::private::RawValue":{payload}}}"#);
        let expected = serde_json::from_str::<serde_json::Value>(&input).expect_err("serde");
        let Err(error) = Document::parse(&input, &controls) else {
            panic!("accepted {input:?}");
        };
        assert_eq!(
            error.test_diagnostic(),
            Some((expected.classify(), expected.line(), expected.column())),
            "{input:?}"
        );
        if expected.classify() == serde_json::error::Category::Data {
            assert_eq!(
                error.test_message(),
                Some(expected.to_string()),
                "{input:?}"
            );
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_raw_source_and_decoded_value_owned_together() {
    // Arrange
    let controls = controls();
    let mut source_ready = None;
    let mut value_ready = None;
    let input = r#"{"$serde_json::private::RawValue":"\"abc\""}"#;

    // Act
    let parsed = Document::parse_observed(input, &controls, |progress| match progress {
        DocumentProgress::RawSourcePushed => {
            source_ready = Some(controls.current_query_memory_bytes());
        }
        DocumentProgress::RawValueReady => {
            value_ready = Some(controls.current_query_memory_bytes());
        }
        _ => {}
    })
    .expect("raw source");

    // Assert
    assert_eq!(
        value_ready.expect("value") - source_ready.expect("source"),
        4
    );
    assert_eq!(
        parsed.forest.text_value(parsed.root).expect("text"),
        Some("abc")
    );
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_nested_raw_source_after_actual_push_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = Document::parse_observed(
        r#"{"$serde_json::private::RawValue":"[1]"}"#,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::RawSourcePushed) {
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_raw_source_and_result_after_actual_value_ready_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = Document::parse_observed(
        r#"{"$serde_json::private::RawValue":"\"abc\""}"#,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::RawValueReady) {
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_raw_source_after_nested_value_admission_is_denied() {
    // Arrange
    let limit = 8192;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut blocker = None;

    // Act
    let result = Document::parse_observed(
        r#"{"$serde_json::private::RawValue":"\"abc\""}"#,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::RawSourcePushed) {
                blocker = Some(
                    controls
                        .reserve_query_memory(limit - controls.current_query_memory_bytes())
                        .expect("blocker"),
                );
            }
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::ResourceLimit(_)))
    ));
    drop(blocker);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_unwind_every_entered_array_while_preserving_the_first_error() {
    // Arrange
    let controls = controls();
    let input = "[[1,]]tail";
    let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
    let mut last_offset = None;

    // Act
    let result = Document::parse_observed(input, &controls, |progress| {
        if let DocumentProgress::UnwindByte(position) = progress {
            last_offset = Some(position.offset);
        }
    });

    // Assert
    let Err(error) = result else {
        panic!("accepted");
    };
    assert_eq!(
        error.test_diagnostic(),
        Some((expected.classify(), expected.line(), expected.column()))
    );
    assert_eq!(last_offset, Some(6));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_unwind_number_sentinel_then_consume_parent_array_comma_and_space() {
    // Arrange
    let controls = controls();
    let input = r#"[{"$serde_json::private::Number":"-xY",   "other":1}]"#;
    let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
    let mut last_offset = None;

    // Act
    let result = Document::parse_observed(input, &controls, |progress| {
        if let DocumentProgress::UnwindByte(position) = progress {
            last_offset = Some(position.offset);
        }
    });

    // Assert
    let Err(error) = result else {
        panic!("accepted");
    };
    assert_eq!(
        error.test_diagnostic(),
        Some((expected.classify(), expected.line(), expected.column()))
    );
    assert_eq!(last_offset, input.find("\"other\""));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_unwind_nested_raw_source_before_wrapping_and_scanning_the_outer_array() {
    // Arrange
    let controls = controls();
    let input = r#"[{"$serde_json::private::RawValue":"[1,]",   "other":1}]"#;
    let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
    let mut last_offset = None;

    // Act
    let result = Document::parse_observed(input, &controls, |progress| {
        if let DocumentProgress::UnwindByte(position) = progress {
            last_offset = Some(position.offset);
        }
    });

    // Assert
    let Err(error) = result else {
        panic!("accepted");
    };
    assert_eq!(
        error.test_diagnostic(),
        Some((expected.classify(), expected.line(), expected.column()))
    );
    assert_eq!(error.test_message(), Some(expected.to_string()));
    assert_eq!(last_offset, input.find("\"other\""));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_report_cancellation_observed_during_post_error_unwinding() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = r#"{"$serde_json::private::Number":"-xY"   }"#;

    // Act
    let result = Document::parse_observed(input, &controls, |progress| {
        if matches!(progress, DocumentProgress::UnwindByte(_)) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_parse_quoted_number_sentinel_with_serde_spelling() {
    // Arrange
    let controls = controls();
    let inputs = [
        r#"{"$serde_json::private::Number":"-0"}"#,
        r#"{"$serde_json::private::Number":"1E2"}"#,
        r#"{"$serde_json::private::Number":"\u0031E2"}"#,
        r#"{"$serde_json::private::Number":"18446744073709551616"}"#,
    ];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input)
            .expect("serde")
            .to_string();
        let parsed = Document::parse(input, &controls).expect("sentinel");
        assert_eq!(
            parsed.forest.number_value(parsed.root).expect("number"),
            Some(expected.as_str())
        );
        drop(parsed);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_attach_a_number_sentinel_inside_an_array() {
    // Arrange
    let controls = controls();
    let input = r#"[{"$serde_json::private::Number":"1E2"}]"#;

    // Act
    let parsed = Document::parse(input, &controls).expect("document");

    // Assert
    let child = parsed
        .forest
        .element(parsed.root, 0)
        .expect("element")
        .expect("number");
    assert_eq!(
        parsed.forest.number_value(child).expect("number"),
        Some("1e+2")
    );
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_number_sentinel_error_position_and_outer_end_precedence() {
    // Arrange
    let controls = controls();
    let inputs = [
        r#"{"$serde_json::private::Number":"-xY","other":1}"#,
        r#"{"$serde_json::private::Number":"1 ","other":1}"#,
        r#"{"$serde_json::private::Number":"1e+xZ","other":1}"#,
        r#"{"$serde_json::private::Number":"","other":1}"#,
        r#"{"$serde_json::private::Number":"\q","other":1}"#,
        r#"{"$serde_json::private::Number":"1","other":1}"#,
        r#"{"$serde_json::private::Number":"1" "other":1}"#,
        r#"{"$serde_json::private::Number"}"#,
    ];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
        let Err(error) = Document::parse(input, &controls) else {
            panic!("accepted {input:?}");
        };
        assert_eq!(
            error.test_diagnostic(),
            Some((expected.classify(), expected.line(), expected.column())),
            "{input:?}"
        );
        if expected.classify() == serde_json::error::Category::Data {
            assert_eq!(
                error.test_number_message(),
                Some(expected.to_string()),
                "{input:?}"
            );
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_nested_number_source_owned_until_strict_scan_completes() {
    // Arrange
    let controls = controls();
    let mut source_ready = None;
    let mut number_ready = None;
    let input = r#"{"$serde_json::private::Number":"12345"}"#;

    // Act
    let parsed = Document::parse_observed(input, &controls, |progress| match progress {
        DocumentProgress::NumberSourceReady => {
            source_ready = Some(controls.current_query_memory_bytes());
        }
        DocumentProgress::NumberDecoded => {
            number_ready = Some(controls.current_query_memory_bytes());
        }
        DocumentProgress::FramePushed
        | DocumentProgress::NumberTypeScanned
        | DocumentProgress::RawSourcePushed
        | DocumentProgress::RawValueReady
        | DocumentProgress::UnwindByte(_) => {}
    })
    .expect("sentinel");

    // Assert
    assert_eq!(
        number_ready.expect("number") - source_ready.expect("source"),
        8
    );
    assert_eq!(
        parsed.forest.number_value(parsed.root).expect("value"),
        Some("12345")
    );
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_nested_number_owners_after_actual_post_decode_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = Document::parse_observed(
        r#"{"$serde_json::private::Number":"12345"}"#,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::NumberDecoded) {
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_nested_source_when_number_output_admission_is_denied() {
    // Arrange
    let limit = 8192;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut blocker = None;

    // Act
    let result = Document::parse_observed(
        r#"{"$serde_json::private::Number":"12345"}"#,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::NumberSourceReady) {
                blocker = Some(
                    controls
                        .reserve_query_memory(limit - controls.current_query_memory_bytes())
                        .expect("blocker"),
                );
            }
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::ResourceLimit(_)))
    ));
    drop(blocker);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_non_string_number_sentinel_type_errors_against_serde() {
    // Arrange
    let controls = controls();
    let payloads = [
        "null",
        "true",
        "false",
        "0",
        "-1",
        "-0",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
        "1.0",
        "1e2",
        "[]",
        "{}",
        "x",
        "truX",
        "1e+x",
    ];

    // Act / Assert
    for payload in payloads {
        let input = format!(r#"{{"$serde_json::private::Number":{payload}}}"#);
        let expected = serde_json::from_str::<serde_json::Value>(&input).expect_err("serde");
        let Err(error) = Document::parse(&input, &controls) else {
            panic!("accepted {input:?}");
        };
        assert_eq!(
            error.test_diagnostic(),
            Some((expected.classify(), expected.line(), expected.column())),
            "{input:?}"
        );
        if expected.classify() == serde_json::error::Category::Data {
            assert_eq!(
                error.test_number_message(),
                Some(expected.to_string()),
                "{input:?}"
            );
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_scanned_number_token_on_actual_type_error_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = Document::parse_observed(
        r#"{"$serde_json::private::Number":18446744073709551616}"#,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::NumberTypeScanned) {
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_drop_partial_forest_on_actual_frame_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = Document::parse_observed("[1]", &controls, |progress| {
        if matches!(progress, DocumentProgress::FramePushed) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_drop_partial_forest_when_later_allocation_is_denied() {
    // Arrange
    let limit = 4096;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut blocker = None;

    // Act
    let result = Document::parse_observed("[1]", &controls, |_| {
        if blocker.is_none() {
            blocker = Some(
                controls
                    .reserve_query_memory(limit - controls.current_query_memory_bytes())
                    .expect("blocker"),
            );
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(DocumentError::Control(CassieError::ResourceLimit(_)))
    ));
    drop(blocker);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
