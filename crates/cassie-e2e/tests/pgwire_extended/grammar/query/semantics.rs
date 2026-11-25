#[test]
fn should_validate_collation_support_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-796-c-collation-order",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'ä' COLLATE \"C\" < 'z' COLLATE \"C\" AS c_order",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("collation Boolean RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16]
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("f".to_owned())]]);

    let unsupported = run_script(
        "issue-796-icu-collation-out-of-scope",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'ä' COLLATE \"und-x-icu\" < 'z' COLLATE \"und-x-icu\"",
        )])],
    );
    assert_eq!(wire::error_code(&unsupported[0]).as_deref(), Some("0A000"));
}

#[test]
fn should_reject_custom_schema_expression_collation_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-796-custom-schema-collation-out-of-scope",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'x' COLLATE other.\"C\" < 'y' COLLATE other.\"C\"",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
    assert_eq!(
        wire::data_rows(result),
        Vec::<Vec<Option<String>>>::new()
    );
}

#[test]
fn should_reject_nested_block_comments_beyond_one_hundred_twenty_eight_levels() {
    // Arrange
    let comment = format!("{}nested{}", "/*".repeat(129), "*/".repeat(129));
    let sql = format!("{comment} SELECT 7::INT4 AS parsed_value");
    let batches = run_script(
        "issue-796-nested-block-comment-depth",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(&sql)])],
    );

    // Act
    let result = &batches[0];
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("54000"));
    assert!(errors.iter().any(|fields| {
        fields.iter().any(|(kind, value)| {
            *kind == 'M' && value == "nested block comment depth exceeds 128"
        })
    }));
    assert!(
        !result.iter().any(|frame| frame.0 == b'T'),
        "resource error precedes metadata"
    );
    assert_eq!(wire::data_rows(result), Vec::<Vec<Option<String>>>::new());
}
