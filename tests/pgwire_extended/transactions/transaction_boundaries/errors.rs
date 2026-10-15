#[test]
fn should_reject_savepoints_in_an_implicit_segment() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("SAVEPOINT implicit_savepoint")];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        Vec::new(),
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("25P01"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_keep_nested_begin_rejected_inside_an_explicit_transaction() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("BEGIN")];
    let tail = vec![vec![wire::simple_query_frame("ROLLBACK")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("0A000"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}
