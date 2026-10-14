//! UUID alternative input syntax exercised through PostgreSQL wire results.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_normalize_alternative_uuid_grouping_forms_over_pgwire() {
    // Arrange
    let sql = "SELECT \
        '{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}'::UUID AS braced, \
        'a0ee-bc99-9c0b-4ef8-bb6d-6bb9-bd38-0a11'::UUID AS four_digit_groups";
    let batches = run_script(
        "issue-816-uuid-alternative-input-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("UUID alternative-input RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["braced", "four_digit_groups"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        [(2950, 16, 0), (2950, 16, 0)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".to_owned()),
            Some("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".to_owned()),
        ]]
    );
}
