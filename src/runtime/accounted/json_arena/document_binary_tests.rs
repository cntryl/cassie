use std::time::Instant;

use super::{binary_json_text, binary_json_text_observed, BinaryJsonError, DocumentProgress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::accounted::json_arena::forest::output::OutputProgress;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_value_mode_output_with_retain_the_output_tracker() {
    // Arrange
    let foreign = controls();
    let controls = controls();
    let input = br#"{"z":[null,true],"a":1E2,"a":-0}"#;
    let expected = serde_json::from_slice::<serde_json::Value>(input)
        .expect("serde")
        .to_string();

    // Act
    let output = binary_json_text(input, &controls).expect("output");
    let (text, memory) = output.into_parts();

    // Assert
    assert_eq!(text, expected);
    assert!(memory.belongs_to(&controls));
    assert!(!memory.belongs_to(&foreign));
    assert!(controls.current_query_memory_bytes() >= text.len());
    drop(text);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(foreign.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_the_previous_reader_for_value_mode_documents_with_failures() {
    // Arrange
    use crate::runtime::accounted::json::controlled_parse::{parse, ParseError};
    let controls = controls();
    let inputs: &[&[u8]] = &[
        br#"{"b":1,"a":2,"b":3}"#,
        br#"["\u0061",1.2300e+5,null]"#,
        br#"{"$serde_json::private::Number":"1E2"}"#,
        br#"{"$serde_json::private::RawValue":"{\"x\":[1,null]}"}"#,
        br#""\uD800""#,
        br#"{"$serde_json::private::Number":"-xY"}"#,
        b"[1,]",
        b"{bad\xff}",
    ];

    // Act / Assert
    for input in inputs {
        let previous = match parse(input, &controls) {
            Ok(value) => Some(value.get().to_string()),
            Err(ParseError::InvalidJson { .. } | ParseError::InvalidUtf8) => None,
            Err(ParseError::Controlled(error)) => panic!("unexpected control error: {error}"),
        };
        let current = match binary_json_text(input, &controls) {
            Ok(value) => Some(value.as_str().to_owned()),
            Err(BinaryJsonError::Invalid) => None,
            Err(BinaryJsonError::Controlled(error)) => panic!("unexpected control error: {error}"),
        };
        // Assert
        assert_eq!(current, previous, "input: {input:?}");
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_invalid_with_control_error_categories() {
    // Arrange
    let controls = controls();
    let invalid: &[&[u8]] = &[
        b"{x\xff}",
        b"[1,]",
        br#"{"$serde_json::private::Number":"-xY"}"#,
        br#"{"$serde_json::private::RawValue":"[1,]"}"#,
    ];
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 0,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());

    // Act / Assert
    for input in invalid {
        // Assert
        assert!(matches!(
            binary_json_text(input, &controls),
            Err(BinaryJsonError::Invalid)
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    assert!(matches!(
        binary_json_text(b"null", &denied),
        Err(BinaryJsonError::Controlled(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_utf8_before_starting_binary_document_parsing() {
    // Arrange
    let controls = controls();
    let mut parser_started = false;

    // Act
    let result =
        binary_json_text_observed(b"[1,]\xff", &controls, |_| parser_started = true, |_| {});

    // Assert
    assert!(matches!(result, Err(BinaryJsonError::Invalid)));
    assert!(!parser_started);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_observe_cancellation_before_invalid_utf8_or_syntax() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );

    // Act / Assert
    for input in [b"{bad\xff}".as_slice(), b"[1,]".as_slice()] {
        // Assert
        assert!(matches!(
            binary_json_text(input, &controls),
            Err(BinaryJsonError::Controlled(CassieError::QueryCancelled))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_parse_with_output_at_measured_peak_with_deny_below() {
    // Arrange
    let input = br#"{"z":[1,true],"a":"x"}"#;
    let baseline = controls();
    let output = binary_json_text(input, &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(output);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    let limits = |budget| CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        ..CassieRuntimeLimits::default()
    };
    let exact = QueryExecutionControls::from_limits(&limits(peak), Instant::now());
    let below = QueryExecutionControls::from_limits(&limits(peak - 1), Instant::now());

    // Act
    let output = binary_json_text(input, &exact).expect("exact peak");
    let denied = binary_json_text(input, &below);

    // Assert
    drop(output);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert!(matches!(
        denied,
        Err(BinaryJsonError::Controlled(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_an_actual_binary_grammar_frame_is_published() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    // Act
    let result = binary_json_text_observed(
        b"[1]",
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::FramePushed) {
                cancellation.cancel();
            }
        },
        |_| {},
    );

    // Assert
    assert!(matches!(
        result,
        Err(BinaryJsonError::Controlled(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_arena_with_output_after_actual_binary_output_copy_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = br#"{"long":"abcdefghijklmnop"}"#;
    let source_memory = controls
        .reserve_query_memory(input.len())
        .expect("source owner");
    let mut admitted_during_copy = 0;
    let mut admitted_during_parse = 0;

    // Act
    let result = binary_json_text_observed(
        input,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::FramePushed) {
                admitted_during_parse = controls.current_query_memory_bytes();
            }
        },
        |progress| {
            if matches!(progress, OutputProgress::SinkCopy) {
                admitted_during_copy = controls.current_query_memory_bytes();
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(admitted_during_parse > input.len());
    assert!(admitted_during_copy > input.len());
    assert!(matches!(
        result,
        Err(BinaryJsonError::Controlled(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), input.len());
    drop(source_memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
