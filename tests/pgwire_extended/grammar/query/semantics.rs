#[test]
fn should_distinguish_icu_collation_order_from_c_order_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-796-non-c-collation-order",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'ä' COLLATE \"C\" < 'z' COLLATE \"C\" AS c_order, \
                'ä' COLLATE \"und-x-icu\" < 'z' COLLATE \"und-x-icu\" AS icu_order",
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
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("f".to_owned()), Some("t".to_owned())]]
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

    // Assert
    let errors = result
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
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
    assert!(wire::data_rows(result).is_empty());
}
