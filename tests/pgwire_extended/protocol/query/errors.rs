#[test]
fn should_return_pg_input_error_info_as_a_star_expanded_row() {
    // Arrange
    let batches = run_script(
        "issue-796-pg-input-error-info-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT * FROM pg_input_error_info('not-an-int', 'integer')",
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
        "pg_input_error_info ErrorResponse: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("pg_input_error_info RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["message", "detail", "hint", "sql_error_code"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 25, 25, 25]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("invalid input syntax for type integer: \"not-an-int\"".to_owned()),
            None,
            None,
            Some("22P02".to_owned()),
        ]]
    );
}
