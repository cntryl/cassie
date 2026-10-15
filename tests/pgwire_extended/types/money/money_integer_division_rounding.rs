//! PostgreSQL 18 money integer-division rounding tracked by #908.

use super::*;

#[test]
fn should_round_money_integer_division_at_signed_half_cent() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET lc_monetary = 'C'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT '1.00'::money / 8 AS positive_half_cent, \
                    ('0.00'::money - '1.00'::money) / 8 AS negative_half_cent",
        )]),
    ];

    // Act
    let batches = run_script("issue-908-money-integer-division-rounding", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money integer-division RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(790, 8, -1), (790, 8, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("$0.12".to_owned()), Some("-$0.12".to_owned()),]]
    );
}
