#[test]
fn should_preserve_promoted_writes_until_explicit_rollback() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("BEGIN; SELECT n FROM wire_tx")];
    let tail = vec![
        vec![wire::sync_frame()],
        vec![wire::simple_query_frame("ROLLBACK")],
    ];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        wire::data_rows(&result.boundary),
        vec![vec![Some("1".to_string())]]
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(result.tail[0], vec![(b'Z', b"T".to_vec())]);
    assert_eq!(
        result.tail[1]
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"CZ"
    );
    assert_eq!(
        result.tail[1].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_preserve_explicit_failed_state_until_rollback() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "SELECT n FROM wire_tx WHERE (n / 0) = 1",
    )];
    let tail = vec![
        vec![wire::sync_frame()],
        vec![wire::simple_query_frame("ROLLBACK")],
    ];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("22012"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(result.tail[0], vec![(b'Z', b"E".to_vec())]);
    assert_eq!(
        result.tail[1].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}
