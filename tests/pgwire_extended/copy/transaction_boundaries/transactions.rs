#[test]
fn should_discard_staged_table_copy_on_explicit_rollback() {
    // Arrange
    let boundary = copy_cycle();
    let tail = vec![vec![wire::simple_query_frame("ROLLBACK")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary), None);
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}
