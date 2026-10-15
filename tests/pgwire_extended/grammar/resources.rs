#[test]
fn should_truncate_identifiers_at_the_utf8_safe_postgres_byte_limit() {
    // Arrange
    let at_limit = "a".repeat(62);
    let before_multibyte = "a".repeat(61);
    let multibyte_within_limit = format!("{before_multibyte}é");
    let sql = format!("SELECT 1 AS \"{at_limit}é\", 2 AS \"{multibyte_within_limit}\"");
    let batches = run_script(
        "issue-796-unicode-identifier-truncation",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(&sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("identifier truncation RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![at_limit.as_str(), multibyte_within_limit.as_str()]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1".to_owned()), Some("2".to_owned())]]
    );
}
