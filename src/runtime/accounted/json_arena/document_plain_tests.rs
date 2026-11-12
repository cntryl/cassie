use super::{Document, DocumentProgress};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_treat_private_sentinel_names_as_ordinary_keys_at_all_depths() {
    // Arrange
    let controls = controls();
    let input = r#"{"$serde_json::private::Number":"7","nested":[{"$serde_json::private::RawValue":"[1]"}]}"#;
    // Act
    let parsed = Document::parse_plain(input, &controls).expect("plain object");
    // Assert
    let number = parsed
        .forest
        .member(parsed.root, "$serde_json::private::Number")
        .expect("lookup")
        .expect("key");
    assert_eq!(parsed.forest.text_value(number).expect("text"), Some("7"));
    let nested = parsed
        .forest
        .member(parsed.root, "nested")
        .expect("lookup")
        .expect("nested");
    let object = parsed
        .forest
        .element(nested, 0)
        .expect("array")
        .expect("element");
    let raw = parsed
        .forest
        .member(object, "$serde_json::private::RawValue")
        .expect("lookup")
        .expect("key");
    assert_eq!(parsed.forest.text_value(raw).expect("text"), Some("[1]"));
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_last_escaped_duplicate_key_in_plain_mode_unlike_value_mode() {
    // Arrange
    let controls = controls();
    let input = r#"{"$serde_json::private::Number":"first","\u0024serde_json::private::Number":"last","after":1}"#;
    // Act
    let plain = Document::parse_plain(input, &controls).expect("plain object");
    let key = plain
        .forest
        .member(plain.root, "$serde_json::private::Number")
        .expect("lookup")
        .expect("key");
    let value_mode =
        Document::parse(r#"{"$serde_json::private::Number":"7"}"#, &controls).expect("value mode");
    // Assert
    assert_eq!(plain.forest.text_value(key).expect("text"), Some("last"));
    assert_eq!(
        value_mode
            .forest
            .number_value(value_mode.root)
            .expect("number"),
        Some("7")
    );
    drop(plain);
    drop(value_mode);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_owners_after_invalid_plain_json() {
    // Arrange
    let controls = controls();
    let invalid = r#"{"$serde_json::private::Number":"7",}"#;
    // Act
    let result = Document::parse_plain(invalid, &controls);
    // Assert
    assert!(result.is_err());
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_plain_object_arena_after_actual_frame_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut frames = 0;
    // Act
    let result = Document::parse_plain_observed(
        r#"{"$serde_json::private::Number":[1]}"#,
        &controls,
        |progress| {
            if matches!(progress, DocumentProgress::FramePushed) {
                frames += 1;
                cancel.cancel();
            }
        },
    );
    // Assert
    assert_eq!(frames, 1);
    assert!(result.is_err());
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_format_plain_sentinel_keys_as_postgresql_jsonb_objects() {
    // Arrange
    let controls = controls();
    let input = r#"{"$serde_json::private::Number":"7","$serde_json::private::RawValue":"[1]"}"#;
    let parsed = Document::parse_plain(input, &controls).expect("plain object");
    // Act
    let output =
        super::super::forest::jsonb_output::serialize(&parsed.forest, parsed.root).expect("output");
    // Assert
    assert_eq!(
        output.as_str(),
        r#"{"$serde_json::private::Number": "7", "$serde_json::private::RawValue": "[1]"}"#
    );
    drop(output);
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_plain_parse_owner_overlap_budget() {
    // Arrange
    let input = r#"{"$serde_json::private::Number":{"nested":[1,2,3]},"other":"payload"}"#;
    let baseline = controls();
    let parsed = Document::parse_plain(input, &baseline).expect("parse");
    let peak = baseline.peak_query_memory_bytes();
    drop(parsed);
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
            let result = Document::parse_plain(input, &controls);
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

#[test]
fn should_report_jsonb_nul_or_numeric_range_errors_after_plain_parsing() {
    // Arrange
    let controls = controls();
    let inputs = [
        (r#"{"$serde_json::private::Number":"\u0000"}"#, true),
        (r#"{"$serde_json::private::Number":1e131072}"#, false),
    ];

    // Act
    let results: Vec<_> = inputs
        .into_iter()
        .map(|(input, nul)| {
            let parsed = Document::parse_plain(input, &controls).expect("plain object");
            let output = super::super::forest::jsonb_output::serialize(&parsed.forest, parsed.root);
            let matched = if nul {
                matches!(
                    output,
                    Err(super::super::forest::jsonb_output::SerializeError::Nul)
                )
            } else {
                matches!(
                    output,
                    Err(super::super::forest::jsonb_output::SerializeError::NumericOverflow)
                )
            };
            drop(parsed);
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
fn should_match_existing_jsonb_format_without_sentinel_keys() {
    // Arrange
    let controls = controls();
    let cases = [r#"{"long":1e2,"a":[null,"é",true]}"#, r#"{"a":1,"a":2}"#];

    // Act
    let results: Vec<_> = cases
        .into_iter()
        .map(|input| {
            let expected = crate::types::jsonb::canonical(input).expect("legacy");
            let parsed = Document::parse_plain(input, &controls).expect("plain object");
            let output = super::super::forest::jsonb_output::serialize(&parsed.forest, parsed.root)
                .expect("output");
            let actual = output.as_str().to_string();
            drop(output);
            drop(parsed);
            (actual, expected, controls.current_query_memory_bytes())
        })
        .collect();

    // Assert
    for (actual, expected, memory) in results {
        assert_eq!(actual, expected);
        assert_eq!(memory, 0);
    }
}
