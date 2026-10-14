//! Later-row JSON_TABLE conversion failures are atomic over pgwire.

use super::{column_type_oids, fixture, wire};

#[test]
fn should_reject_later_json_table_conversion_without_partial_rows() {
    // Arrange
    let malformed_later_row = fixture::execute_cycle(
        "SELECT id, n FROM JSON_TABLE( \
           '[{\"id\":1,\"n\":1},{\"id\":2,\"n\":\"bad\"}]'::JSONB, \
           '$[*]' COLUMNS ( \
             id INTEGER PATH '$.id', \
             n INTEGER PATH '$.n' ERROR ON ERROR \
           ) \
         ) AS rows",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-829-json-table-later-row-conversion-error",
        &[],
        vec![malformed_later_row, recovery],
    );

    // Assert
    let error_frames = &batches[0];
    assert_eq!(wire::error_code(error_frames).as_deref(), Some("22P02"));
    assert_eq!(column_type_oids(error_frames), vec![23, 23]);
    assert!(wire::data_rows(error_frames).is_empty());
    assert_eq!(
        error_frames.iter().filter(|(tag, _)| *tag == b'C').count(),
        0
    );
    assert_eq!(error_frames.last().map(|frame| frame.0), Some(b'Z'));

    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}
