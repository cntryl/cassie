//! PostgreSQL modulo-by-zero SQLSTATE with bound int4 operands.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_report_modulo_by_zero_for_bound_int4_parameters() {
    // Arrange
    let statement = "mod_zero_division";
    let portal = "mod_zero_division_portal";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT MOD($1, $2) AS remainder",
            &[23, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["9", "0"]),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("issue-813-bound-mod-zero", steps);
    let described = &batches[1];
    let failed = &batches[2];
    let recovery = &batches[3];

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(wire::error_code(described), None);
    let parameter_types = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("int4 ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![23, 23]
    );
    let result_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("int4 MOD RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        23
    );
    assert_eq!(wire::error_code(failed).as_deref(), Some("22012"));
    let error = failed
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("MOD division-by-zero ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(kind, message)| { *kind == 'M' && message == "division by zero" }));
    assert!(!failed
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("42".to_owned())]]);
}
