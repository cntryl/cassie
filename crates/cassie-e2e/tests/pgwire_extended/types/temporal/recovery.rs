#[test]
fn should_recover_after_temporal_input_error_on_the_same_connection() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('2024-02-30 12:00:00' AS TIMESTAMP)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TIMESTAMP '2024-02-29 12:00:00'",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-error-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22008"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("2024-02-29 12:00:00".to_owned())]]
    );
}
