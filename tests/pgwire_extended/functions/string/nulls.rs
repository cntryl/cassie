#[test]
fn should_report_null_value_not_allowed_for_null_identifier_format() {
    // Arrange
    let batches = run_script(
        "issue-803-format-null-identifier-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', 'identifier')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', NULL::TEXT)",
            )]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(
        wire::data_rows(valid),
        vec![vec![Some("identifier".to_owned())]]
    );
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22004"));
}

#[test]
fn should_preserve_left_rows_when_lateral_unnest_is_empty() {
    // Arrange
    let sql = "SELECT source.id, expanded.value, expanded.ordinality FROM (VALUES (1, ARRAY[10,20]::int4[]), (2, ARRAY[]::int4[])) AS source(id, vals) LEFT JOIN LATERAL unnest(source.vals) WITH ORDINALITY AS expanded(value, ordinality) ON TRUE ORDER BY source.id, expanded.ordinality";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-left-join-empty-unnest", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("left lateral UNNEST RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("10".to_owned()),
                Some("1".to_owned()),
            ],
            vec![
                Some("1".to_owned()),
                Some("20".to_owned()),
                Some("2".to_owned()),
            ],
            vec![Some("2".to_owned()), None, None],
        ]
    );
}
