#[test]
fn should_stop_returning_generate_series_rows_at_limit() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT value FROM generate_series(1, 10000) AS series(value) LIMIT 2",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-808-generate-series-limit", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
}
