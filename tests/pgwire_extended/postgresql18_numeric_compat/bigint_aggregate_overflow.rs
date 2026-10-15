use super::{run_script, wire, Step};

#[test]
fn should_preserve_overflowing_bigint_aggregates_over_wire() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_bigint_overflow_aggregates \
             (bucket INTEGER, value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_bigint_overflow_aggregates VALUES \
             (1, 9223372036854775807), (1, 1), \
             (2, -9223372036854775808), (2, -1)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT bucket, SUM(value), AVG(value) \
             FROM issue_812_bigint_overflow_aggregates \
             GROUP BY bucket ORDER BY bucket",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-bigint-aggregate-overflow", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None, "table creation");
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "boundary row insertion"
    );
    let result = &batches[2];
    assert_eq!(
        wire::error_code(result),
        None,
        "aggregate query error fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("bigint aggregate overflow RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1700, -1, -1), (1700, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("9223372036854775808".to_owned()),
                Some("4611686018427387904".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("-9223372036854775809".to_owned()),
                Some("-4611686018427387905".to_owned()),
            ],
        ]
    );
}
