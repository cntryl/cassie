//! Geometry operator result identities when an operand is SQL NULL.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_operator_result_types_given_null_geometry_operands_when_results_are_described() {
    // Arrange
    let sql = "SELECT NULL::point + point '(1,2)' AS translated, \
                      NULL::point <-> point '(1,2)' AS distance, \
                      NULL::circle @> point '(1,2)' AS contains";

    // Act
    let batches = run_script(
        "issue-907-null-geometry-operator-types",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("geometry operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [600, 701, 16]
    );
    assert_eq!(wire::data_rows(result), vec![vec![None, None, None]]);
}
