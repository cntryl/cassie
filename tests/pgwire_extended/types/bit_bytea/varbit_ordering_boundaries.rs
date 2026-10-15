//! PostgreSQL 18 variable bit-string ordering boundaries tracked by #816.

use super::*;

#[test]
fn should_order_varbit_values_by_bits_then_length_over_pgwire() {
    // Arrange
    let sql = "SELECT value FROM (VALUES \
        (B'1'::varbit), (B'01'::varbit), (B'001'::varbit), \
        (B'0010'::varbit), (B'000'::varbit), (B'0000'::varbit) \
    ) AS values_to_order(value) ORDER BY value";
    let batches = run_script(
        "issue-816-varbit-ordering-boundaries",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code
            ))
            .collect::<Vec<_>>(),
        vec![(1562, -1, -1, 0)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("000".to_owned())],
            vec![Some("0000".to_owned())],
            vec![Some("001".to_owned())],
            vec![Some("0010".to_owned())],
            vec![Some("01".to_owned())],
            vec![Some("1".to_owned())],
        ]
    );
}
