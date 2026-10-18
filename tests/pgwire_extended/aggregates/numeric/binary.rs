//! PostgreSQL 18 NUMERIC OID, typmod and codec contracts tracked by #812.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "aggregate_special_values.rs"]
mod aggregate_special_values;
#[path = "bigint_aggregate_overflow.rs"]
mod bigint_aggregate_overflow;
#[path = "numeric_aggregate_fraction.rs"]
mod numeric_aggregate_fraction;
#[path = "../../types/numeric/numeric_typmod_domain_edges.rs"]
mod numeric_typmod_domain_edges;

#[test]
fn should_encode_bigint_aggregate_results_as_numeric_in_binary_format() {
    // Arrange
    let statement = "bigint_aggregate_binary_results";
    let portal = "bigint_aggregate_binary_portal";
    let sql = "SELECT SUM(value), AVG(value) FROM issue_812_bigint_binary_aggregates";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_bigint_binary_aggregates (value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_bigint_binary_aggregates VALUES (9007199254740992), (9007199254740993)",
        )]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-bigint-aggregate-binary-results", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(wire::error_code(&batches[3]), None);
    assert_eq!(wire::error_code(&batches[4]), None);
    let statement_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared aggregate RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    let statement_contract = statement_columns
        .iter()
        .map(|column| {
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            )
        })
        .collect::<Vec<_>>();

    let binary_description = batches[4]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary aggregate portal RowDescription");
    let binary_columns = wire::parse_row_description(&binary_description.1);
    let binary_contract = binary_columns
        .iter()
        .map(|column| {
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            )
        })
        .collect::<Vec<_>>();
    let sum = [
        0x00, 0x05, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x1f, 0x4e, 0x0f, 0x91, 0x03,
        0xb4, 0x07, 0xc1,
    ];
    let average = [
        0x00, 0x05, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x23, 0x2f, 0x07, 0xc8, 0x15, 0x62, 0x03,
        0xe0, 0x13, 0x88,
    ];
    let mut expected_row = vec![0x00, 0x02];
    for value in [&sum[..], &average[..]] {
        expected_row.extend_from_slice(
            &i32::try_from(value.len())
                .expect("numeric wire value length fits i32")
                .to_be_bytes(),
        );
        expected_row.extend_from_slice(value);
    }
    let binary_rows = crate::support_type_metadata_contract::data_row_payloads(&batches[4]);
    assert_eq!(
        (statement_contract, binary_contract, binary_rows),
        (
            vec![(1700, -1, -1, 0), (1700, -1, -1, 0)],
            vec![(1700, -1, -1, 1), (1700, -1, -1, 1)],
            vec![expected_row],
        ),
        "prepared and binary portal metadata with SUM/AVG DataRow bytes"
    );
}

include!("metadata.rs");

include!("semantics.rs");

include!("../../protocol/numeric/errors.rs");

include!("../../types/numeric/binary.rs");

include!("../../types/numeric/errors.rs");

include!("../../types/numeric/resources.rs");
