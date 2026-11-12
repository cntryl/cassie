use super::{Document, DocumentProgress};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_slice_read_document_error_category_and_position() {
    // Arrange
    let controls = controls();
    let inputs: &[&[u8]] = &[
        b"[}\xff",
        b"\xff",
        b"{}\xff",
        b"{\"x\":\"\xff\\q\"}",
        b"{\"\xff\\q\":1}",
        b"{\n \"a\":0,\n \"\xff\\nX\":1}",
        b"[\"\xff\x01\"]",
        b"[\"\xff",
        b"[1,]",
        b"{\"a\":1,}",
        b"{\"a\":}",
        b"1e",
    ];
    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_slice::<serde_json::Value>(input).expect_err("legacy");
        let Err(actual) = Document::parse_plain_row(input, &controls) else {
            panic!("accepted malformed document");
        };
        assert_eq!(
            actual.test_diagnostic(),
            Some((expected.classify(), expected.line(), expected.column())),
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_plain_sentinel_keys_while_copy_value_mode_is_unchanged() {
    // Arrange
    let controls = controls();
    let input = br#"{"$serde_json::private::Number":"7"}"#;
    // Act
    let row = Document::parse_plain_row(input, &controls).expect("row document");
    let value =
        Document::parse(std::str::from_utf8(input).expect("UTF-8"), &controls).expect("value mode");
    // Assert
    let key = row
        .forest
        .member(row.root, "$serde_json::private::Number")
        .expect("lookup")
        .expect("key");
    assert_eq!(row.forest.text_value(key).expect("text"), Some("7"));
    assert_eq!(
        value.forest.number_value(value.root).expect("number"),
        Some("7")
    );
    drop(row);
    drop(value);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_slice_read_for_valid_nonsentinel_documents() {
    // Arrange
    let controls = controls();
    let inputs = [
        r#"{"a":[null,true,"é",1E2],"a":{"b":"x"}}"#,
        r#"["",{},false]"#,
    ];
    // Act / Assert
    for input in inputs {
        let expected: serde_json::Value = serde_json::from_slice(input.as_bytes()).expect("legacy");
        let row = Document::parse_plain_row(input.as_bytes(), &controls).expect("row");
        let output =
            super::super::forest::output::serialize(&row.forest, row.root).expect("output");
        assert_eq!(output.as_str(), expected.to_string());
        drop(output);
        drop(row);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_row_document_after_actual_frame_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut frames = 0;
    // Act
    let result = Document::parse_plain_row_observed(br#"{"a":[1]}"#, &controls, |progress| {
        if matches!(progress, DocumentProgress::FramePushed) {
            frames += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(frames, 1);
    assert!(result.is_err());
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
