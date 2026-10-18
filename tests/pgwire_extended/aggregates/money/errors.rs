#[test]
fn should_report_money_sum_overflow_with_numeric_value_out_of_range_sqlstate() {
    // Arrange
    let overflow = "SELECT SUM(amount) FROM (VALUES ('92233720368547758.07'::money), ('0.01'::money)) AS valueset(amount)";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(overflow)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 47")]),
    ];

    // Act
    let batches = run_script("issue-908-money-sum-overflow", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("47".to_owned())]]
    );
}
