#[test]
fn should_stage_table_copy_until_explicit_commit() {
    // Arrange
    let boundary = copy_cycle();
    let tail = vec![vec![wire::simple_query_frame("COMMIT")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT n FROM wire_tx ORDER BY n",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary), None);
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"GCZ"
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("1".to_string())], vec![Some("2".to_string())]]
    );
}
