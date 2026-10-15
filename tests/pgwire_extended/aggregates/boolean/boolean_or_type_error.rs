//! Boolean aggregate argument typing over the PostgreSQL wire protocol.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_reject_integer_bool_or_with_undefined_function_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-805-bool-or-integer-argument-type",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT BOOL_OR(1)")]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let invalid = &batches[0];
    let recovery = &batches[1];

    // Assert
    assert_eq!(wire::error_code(invalid).as_deref(), Some("42883"));
    let error = invalid
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("undefined BOOL_OR overload ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, message)| message.as_str()),
        Some("function bool_or(integer) does not exist")
    );
    assert_eq!(wire::data_rows(invalid), Vec::<Vec<Option<String>>>::new());
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}
