#[test]
fn should_preserve_power_precedence_with_float8_metadata() {
    // Arrange
    let batches = run_script(
        "issue-796-power-operator-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 2 ^ 3 ^ 2 AS left_associative_power, \
                2 * 3 ^ 2 AS exponent_before_multiplication",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("power operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("64".to_owned()), Some("18".to_owned())]]
    );
}
