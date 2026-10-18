//! Uncovered JSON versus JSONB inspection and conversion contracts for #810.

use super::super::{fixture, wire};

fn column_signatures(frames: &fixture::Frames) -> Vec<(i32, i16, i32)> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| (column.type_oid, column.type_size, column.type_mod))
        .collect()
}

#[test]
fn should_reject_nonarray_json_array_length_inputs_with_22023() {
    // Arrange
    let json = fixture::execute_cycle("SELECT json_array_length('{}'::JSON)", None, 0, true);
    let jsonb = fixture::execute_cycle("SELECT jsonb_array_length('{}'::JSONB)", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-json-array-length-nonarray-error",
        &[],
        vec![json, jsonb, recovery],
    );

    // Assert
    for frames in &batches[..2] {
        assert_eq!(wire::error_code(frames).as_deref(), Some("22023"));
        assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
    }
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_reject_jsonb_set_lax_null_raise_without_partial_rows() {
    // Arrange
    let malformed = fixture::execute_cycle(
        "SELECT jsonb_set_lax('{\"a\":1}'::JSONB, '{a}', NULL::JSONB, TRUE, 'raise_exception'), 42",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-set-lax-raise-exception",
        &[],
        vec![malformed, recovery],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22004"));
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

include!("nulls.rs");

include!("semantics.rs");
