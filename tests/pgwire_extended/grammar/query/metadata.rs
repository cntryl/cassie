#[test]
fn should_select_postgres_integer_literal_oids_at_the_signed_boundary() {
    // Arrange
    let batches = run_script(
        "issue-796-integer-literal-width-boundary",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 2147483647 AS int4_max, 2147483648 AS int8_min",
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
        "integer literal boundary ErrorResponse: {errors:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("integer literal boundary RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        [("int4_max", 23), ("int8_min", 20)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("2147483647".to_owned()),
            Some("2147483648".to_owned()),
        ]]
    );
}
