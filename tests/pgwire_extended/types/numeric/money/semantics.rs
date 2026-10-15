#[test]
fn should_preserve_money_numeric_operator_types() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET lc_monetary = 'C'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 10.50::numeric::money - 3.25::numeric::money AS difference, \
                    10.50::numeric::money * 2.5::numeric AS money_times_numeric, \
                    2.5::numeric * 10.50::numeric::money AS numeric_times_money, \
                    10.50::numeric::money / 4::numeric AS divided",
        )]),
    ];

    // Act
    let batches = run_script("issue-908-money-numeric-operator-types", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money numeric-operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(790, 8, -1), (790, 8, -1), (790, 8, -1), (790, 8, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("$7.25".to_owned()),
            Some("$26.25".to_owned()),
            Some("$26.25".to_owned()),
            Some("$2.62".to_owned()),
        ]]
    );
}
