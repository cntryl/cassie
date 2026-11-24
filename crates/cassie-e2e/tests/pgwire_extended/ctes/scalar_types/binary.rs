//! PostgreSQL 18 scalar type inference contracts tracked by #817.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_infer_bigint_oid_zero_parameter_across_binary_cycle() {
    // Arrange
    let statement = "coalesce_oid_zero_bigint";
    let portal = "coalesce_oid_zero_bigint_portal";
    let sql = "SELECT COALESCE($1, NULL::BIGINT) AS chosen";
    let input = 41_i64.to_be_bytes();
    let expected = 41_i64.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[0])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[1], &[Some(&input)], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-oid-zero-coalesce-bigint", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );

    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("inferred ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![20]);
    let statement_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("COALESCE statement RowDescription");
    let column = &wire::parse_row_description(&statement_description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (20, 8, -1)
    );

    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    let portal_description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary portal RowDescription");
    let column = &wire::parse_row_description(&portal_description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (20, 8, -1)
    );
    assert_eq!(column.format_code, 1);
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&expected)]
    );
}
