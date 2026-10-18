#[test]
fn should_keep_grouped_bigint_aggregate_types_numeric() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_grouped_bigint_values (bucket INTEGER, value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_grouped_bigint_values VALUES (1, 9007199254740992), (1, 9007199254740993), (2, 7), (2, 8)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE INDEX issue_812_grouped_bigint_bucket_value_idx ON issue_812_grouped_bigint_values (bucket, value)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT bucket, SUM(value), AVG(value) FROM issue_812_grouped_bigint_values WHERE bucket IN (1, 2) GROUP BY bucket ORDER BY bucket",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-grouped-bigint-aggregates", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    let description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("grouped aggregate RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1700, -1, -1), (1700, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![
            vec![
                Some("1".to_owned()),
                Some("18014398509481985".to_owned()),
                Some("9007199254740992.5000".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("15".to_owned()),
                Some("7.5000000000000000".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_preserve_exact_numeric_aggregate_values_beyond_float_precision() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_numeric_aggregate_values (value NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_numeric_aggregate_values VALUES (9007199254740993.1255), (0.0001)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value) FROM issue_812_numeric_aggregate_values",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-aggregate-precision", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[2]), None);
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700],
        "aggregate values: {:?}",
        wire::data_rows(&batches[2])
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("9007199254740993.1256".to_owned()),
            Some("4503599627370496.5628".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_numeric_aggregates_beyond_thirty_eight_digits() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_wide_numeric_aggregates (value NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_wide_numeric_aggregates VALUES \
             ('123456789012345678901234567890123456789012345678.90'), (0.11)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value), MAX(value) + 0.11, MAX(value) * 2 \
             FROM issue_812_wide_numeric_aggregates",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-wide-numeric-aggregates", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("wide numeric aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1700, -1, -1); 4]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("123456789012345678901234567890123456789012345679.01".to_owned()),
            Some("61728394506172839450617283945061728394506172839.51".to_owned()),
            Some("123456789012345678901234567890123456789012345679.01".to_owned()),
            Some("246913578024691357802469135780246913578024691357.80".to_owned()),
        ]]
    );
}
