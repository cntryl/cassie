//! JOIN USING is outside Cassie's currently selected SQL profile.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn assert_error(
    frames: &[(u8, Vec<u8>)],
    expected_state: &str,
    expected_message: &str,
    context: &str,
) {
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some(expected_state),
        "{context} ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let fields = wire::parse_error_fields(
        &frames
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("expected JOIN USING ErrorResponse")
            .1,
    );
    assert!(
        fields
            .iter()
            .any(|(kind, value)| *kind == 'M' && value == expected_message),
        "{context} ErrorResponse fields: {fields:?}"
    );
}

#[test]
fn should_reject_join_using_forms_outside_the_selected_sql_profile() {
    // Arrange
    let queries = [
        "SELECT id FROM left_table JOIN right_table USING (id)",
        "SELECT id FROM left_table JOIN right_table USING (id, id)",
        "SELECT id FROM left_table JOIN right_table USING (missing)",
    ];
    let steps = queries
        .iter()
        .map(|sql| {
            Step::ThroughReady(vec![wire::simple_query_frame(sql)])
        })
        .collect();
    let batches = run_script("join-using-unselected", steps);

    // Act
    let results = batches;

    // Assert
    assert_eq!(results.len(), queries.len());
    for (sql, frames) in queries.into_iter().zip(results) {
        assert_error(
            &frames,
            "42601",
            "JOIN requires ON predicate",
            sql,
        );
        assert!(wire::data_rows(&frames).is_empty(), "{sql}");
        assert!(!frames.iter().any(|frame| frame.0 == b'T'), "{sql}");
    }
}
