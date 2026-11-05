//! Dynamic SIMILAR TO parameter identity, translation output and escape errors.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_dynamic_similar_parameters_with_boolean_text_results() {
    // Arrange
    let sql = "SELECT $1 SIMILAR TO $2 ESCAPE $3 AS matched, pg_catalog.similar_to_escape($2, $3) AS translated";
    let batches = run_script(
        "issue-799-dynamic-similar-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types("similar", sql, &[0, 0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("similar"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("similar_portal", "similar", &["a_b", "a!_b", "!"]),
                wire::describe_portal_frame("similar_portal"),
                wire::execute_frame("similar_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    let parameters = &described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription")
        .1;
    assert_eq!(
        wire::parse_parameter_description(parameters),
        vec![25, 25, 25]
    );
    for batch in [described, executed] {
        let columns = wire::parse_row_description(
            &batch
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("RowDescription")
                .1,
        );
        assert_eq!(
            columns
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![16, 25]
        );
    }
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("t".into()), Some(r"^(?:a\_b)$".into())]]
    );
}

#[test]
fn should_preserve_dynamic_similar_escape_error_hint_after_null_left_input() {
    // Arrange
    let batches = run_script(
        "issue-799-dynamic-similar-escape-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "bad_escape",
                "SELECT NULL SIMILAR TO $1 ESCAPE $2",
                &[0, 0],
            )),
            Step::ThroughReady(vec![
                wire::bind_frame("bad_escape_portal", "bad_escape", &["x", "xx"]),
                wire::execute_frame("bad_escape_portal"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 'x' SIMILAR TO 'x'")]),
        ],
    );

    // Act
    let rejected = &batches[1];
    let retry = &batches[2];

    // Assert
    assert_eq!(wire::error_code(rejected).as_deref(), Some("22025"));
    let fields = wire::parse_error_fields(
        &rejected
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("ErrorResponse")
            .1,
    );
    assert!(fields
        .iter()
        .any(|(kind, value)| *kind == 'H'
            && value == "Escape string must be empty or one character."));
    assert_eq!(wire::data_rows(rejected), Vec::<Vec<Option<String>>>::new());
    assert!(!rejected.iter().any(|frame| frame.0 == b'C'));
    assert_eq!(wire::error_code(retry), None);
    assert_eq!(wire::data_rows(retry), vec![vec![Some("t".into())]]);
}
