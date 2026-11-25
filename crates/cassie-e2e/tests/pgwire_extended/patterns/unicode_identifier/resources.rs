//! Positive PostgreSQL Unicode-escaped identifier contracts for #796.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_decode_custom_unicode_escape_in_delimited_identifier() {
    // Arrange
    let batches = run_script(
        "issue-796-unicode-escaped-delimited-identifier",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 1::INT4 AS U&\"d!0061t\" UESCAPE '!'",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(result),
        None,
        "Unicode-escaped identifier ErrorResponse: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("Unicode-escaped identifier RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns[0].name, "dat");
    assert_eq!(columns[0].type_oid, 23);
    assert_eq!(wire::data_rows(result), vec![vec![Some("1".to_owned())]]);
}
