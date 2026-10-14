//! Rejection behavior for malformed multidimensional array constructors.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_reject_ragged_multidimensional_array_constructor_with_exact_error() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ARRAY[[1], [2]]::int4[] AS rectangular",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT ARRAY[[1], [2,3]]")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 47 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-ragged-multidimensional-array", steps);

    // Assert
    assert_eq!(batches.len(), 3);
    let valid = &batches[0];
    assert_eq!(wire::error_code(valid), None);
    let descriptor = valid
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("rectangular array RowDescription");
    let column = &wire::parse_row_description(&descriptor.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (1007, -1, -1)
    );
    assert_eq!(
        wire::data_rows(valid),
        vec![vec![Some("{{1},{2}}".to_owned())]]
    );

    let invalid = &batches[1];
    assert_eq!(wire::error_code(invalid).as_deref(), Some("2202E"));
    let fields = invalid
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("ragged array constructor ErrorResponse");
    assert!(
        fields.iter().any(|(kind, value)| {
            *kind == 'M'
                && value
                    == "multidimensional arrays must have array expressions with matching dimensions"
        }),
        "ragged array constructor message: {fields:?}"
    );
    assert!(wire::data_rows(invalid).is_empty());

    let recovery = &batches[2];
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("47".to_owned())]]);
}
