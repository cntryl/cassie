#[test]
fn should_report_invalid_ordered_set_percentile_fraction_sqlstate_over_pgwire() {
    // Arrange
    let batches = run_invalid_percentile_and_recovery();

    // Act
    let error = batches.first().expect("invalid percentile response");
    let recovery = batches.get(1).expect("post-error recovery response");
    let message = error
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, body)| {
            wire::parse_error_fields(body)
                .into_iter()
                .find_map(|(key, value)| (key == 'M').then_some(value))
        });

    // Assert
    assert_eq!(wire::error_code(error).as_deref(), Some("22003"));
    assert_eq!(
        message.as_deref(),
        Some("percentile value -0.1 is not between 0 and 1")
    );
    assert!(
        error
            .iter()
            .all(|(tag, _)| !matches!(*tag, b'T' | b'D' | b'C')),
        "an invalid percentile must not publish result frames: {error:?}"
    );
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("42".to_owned())]]);
}
