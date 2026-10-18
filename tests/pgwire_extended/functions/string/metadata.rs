#[test]
fn should_return_parse_ident_text_array_with_postgres_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-parse-ident-wire-result",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT parse_ident('\"Foo\".bar')",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("parse_ident RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 1009);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("{Foo,bar}".to_owned())]]
    );
}

#[test]
fn should_return_string_to_table_rows_with_ordinality_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-string-to-table-wire-results",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT value, ordinal FROM string_to_table('a,,b', ',', '') WITH ORDINALITY AS t(value, ordinal) ORDER BY ordinal",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("string_to_table RowDescription")
            .1,
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("a".to_owned()), Some("1".to_owned())],
            vec![None, Some("2".to_owned())],
            vec![Some("b".to_owned()), Some("3".to_owned())],
        ]
    );
}
