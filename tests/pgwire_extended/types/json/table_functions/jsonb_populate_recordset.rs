//! JSONB populate-recordset later-row errors without a blocking sort.

use super::super::{fixture, wire};

#[test]
fn should_reject_later_jsonb_populate_recordset_conversion_without_partial_rows() {
    // Arrange
    let invalid = fixture::execute_cycle(
        "SELECT f1, f2 FROM jsonb_populate_recordset(ROW(5::BIGINT, 'base'::TEXT), '[{\"f1\":6,\"f2\":\"valid\"},{\"f1\":\"bad\",\"f2\":\"later\"}]'::JSONB) AS records(f1 BIGINT, f2 TEXT)",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-829-jsonb-populate-recordset-later-error",
        &[],
        vec![invalid, recovery],
    );

    // Assert
    let error_frames = &batches[0];
    assert_eq!(wire::error_code(error_frames).as_deref(), Some("22P02"));
    let errors = error_frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("record conversion error message");
    assert_eq!(message, "invalid input syntax for type bigint: \"bad\"");
    assert_eq!(
        error_frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .map(|(_, body)| {
                wire::parse_row_description(body)
                    .iter()
                    .map(|column| column.type_oid)
                    .collect::<Vec<_>>()
            }),
        Some(vec![20, 25])
    );
    assert_eq!(
        wire::data_rows(error_frames),
        Vec::<Vec<Option<String>>>::new()
    );
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
