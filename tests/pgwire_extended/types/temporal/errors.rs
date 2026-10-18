#[test]
fn should_return_datetime_error_sqlstate_for_invalid_timestamp_fields() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT CAST('2024-02-30 12:00:00' AS TIMESTAMP)",
    )])];

    // Act
    let batches = run_script("postgresql18-temporal-invalid-field", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22008"));
}

#[test]
fn should_report_datetime_sqlstates_for_malformed_temporal_values() {
    // Arrange
    let cases = [
        ("date_field", "SELECT CAST('2024-02-30' AS DATE)", "22008"),
        ("time_field", "SELECT CAST('24:00:01' AS TIME)", "22008"),
        (
            "timetz_displacement",
            "SELECT CAST('12:00:00+25:00' AS TIMETZ)",
            "22009",
        ),
        (
            "interval_input",
            "SELECT CAST('not an interval' AS INTERVAL)",
            "22007",
        ),
    ];
    let steps = cases
        .iter()
        .map(|(_, sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("postgresql18-temporal-invalid-input-matrix", steps);

    // Assert
    for (batch, (case, _, expected)) in batches.iter().zip(cases) {
        assert_eq!(
            wire::error_code(batch).as_deref(),
            Some(expected),
            "{case}: {:?}",
            batch
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
    }
}
